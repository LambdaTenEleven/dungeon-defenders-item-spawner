//! A thread-specific message hook. Only this callback may enter the engine.

use dd_protocol::{
    Command, ErrorCode, Rejection, Request, Response, SpawnKind, SpawnOptions, SpawnOutcome,
    VERSION, pipe::Pipe, read_frame, write_frame,
};
use std::{
    io::{self, Read},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, WAIT_TIMEOUT, WPARAM},
    System::{
        LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN,
            GetModuleHandleExW,
        },
        Threading::{GetCurrentThreadId, OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject},
    },
    UI::WindowsAndMessaging::{
        CallNextHookEx, HC_ACTION, MSG, PM_REMOVE, PostThreadMessageW, RegisterWindowMessageW,
        WM_NULL,
    },
};

pub(crate) const START: isize = 1;
pub(crate) const WAKE: isize = 2;
pub(crate) const STOP: isize = 3;
const QUEUED: u8 = 0;
const RUNNING: u8 = 1;
const CANCELLED: u8 = 2;
const SCHEDULE_TIMEOUT: Duration = Duration::from_millis(750);
const RESPONSE_RESERVE: Duration = Duration::from_millis(250);

static SESSION: Mutex<Option<Arc<Session>>> = Mutex::new(None);
static MESSAGE: OnceLock<u32> = OnceLock::new();

pub(crate) fn message_id() -> u32 {
    *MESSAGE.get_or_init(|| {
        let name: Vec<u16> = "DungeonWorkshop.Adapter.Message.v1"
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // SAFETY: name is NUL terminated and readable until the call returns.
        unsafe { RegisterWindowMessageW(name.as_ptr()) }
    })
}

struct Session {
    owner: OwnedHandle,
    owner_pid: u32,
    thread: u32,
    jobs: Mutex<Receiver<Job>>,
    active: AtomicBool,
    finished: AtomicBool,
}

impl Session {
    fn alive(&self) -> bool {
        self.active.load(Ordering::Acquire)
            // SAFETY: owner remains an owned, valid process handle for this session.
            && unsafe { WaitForSingleObject(self.owner.as_raw_handle(), 0) } == WAIT_TIMEOUT
    }
}

struct Job {
    request: Request,
    deadline: Instant,
    state: Arc<AtomicU8>,
    response: SyncSender<Response>,
    catalog: Option<Arc<dd_catalog::CatalogSnapshot>>,
}

impl Job {
    fn begin(&self) -> bool {
        if Instant::now() >= self.deadline {
            let _ =
                self.state
                    .compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire);
            return false;
        }
        self.state
            .compare_exchange(QUEUED, RUNNING, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }
}

fn reject(id: u64, message: &str) -> Response {
    Response {
        version: VERSION,
        id,
        result: Err(Rejection {
            code: ErrorCode::IntegrationUnavailable,
            message: message.into(),
            spawn: None,
        }),
    }
}

/// Checks helper ownership without waiting on a reentrant message-hook lock.
pub(crate) fn attachment_active() -> bool {
    SESSION
        .try_lock()
        .ok()
        .and_then(|session| session.clone())
        .is_some_and(|session| session.alive())
}

fn uncertain_spawn(request: &Request) -> Option<SpawnOutcome> {
    match &request.command {
        Command::Spawn {
            quantity, options, ..
        } => Some(SpawnOutcome {
            requested: *quantity,
            spawned: 0,
            kind: if matches!(options, SpawnOptions::WorldPickup {}) {
                SpawnKind::WorldPickup
            } else {
                SpawnKind::EquipmentPickup
            },
            uncertain: true,
        }),
        _ => None,
    }
}

fn reject_started(id: u64, message: &str, spawn: Option<SpawnOutcome>) -> Response {
    let mut response = reject(id, message);
    if let Err(rejection) = &mut response.result {
        rejection.spawn = spawn;
    }
    response
}

fn completion_budget(deadline: Instant, now: Instant) -> Duration {
    deadline.saturating_duration_since(now)
}

fn begin(owner_pid: u32) {
    if owner_pid == 0 || owner_pid == std::process::id() || usize::BITS != 32 {
        return;
    }
    let Ok(mut slot) = SESSION.try_lock() else {
        return;
    };
    if slot
        .as_ref()
        .is_some_and(|session| !session.finished.load(Ordering::Acquire))
    {
        return;
    }
    // SAFETY: only synchronization rights are requested; result is checked and owned below.
    let owner = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, owner_pid) };
    if owner.is_null() {
        return;
    }
    // SAFETY: OpenProcess returned a uniquely owned non-null process handle.
    let owner = unsafe { OwnedHandle::from_raw_handle(owner) };
    // SAFETY: owner is a valid process handle. Ignore a bootstrap left queued after its host exited.
    if unsafe { WaitForSingleObject(owner.as_raw_handle(), 0) } != WAIT_TIMEOUT {
        return;
    }
    let mut module = std::ptr::null_mut();
    // SAFETY: the address belongs to this DLL; PIN keeps callbacks/workers valid until game exit.
    if unsafe {
        GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_PIN,
            dd_adapter_hook as *const () as *const u16,
            &mut module,
        )
    } == 0
    {
        return;
    }
    // Prewarm reflection on the game thread. The engine retains any error for its status response.
    let _ = crate::engine::initialize();
    let (sender, receiver) = mpsc::sync_channel(1);
    let session = Arc::new(Session {
        owner,
        owner_pid,
        // SAFETY: this is a parameter-free query of the executing hook thread.
        thread: unsafe { GetCurrentThreadId() },
        jobs: Mutex::new(receiver),
        active: AtomicBool::new(true),
        finished: AtomicBool::new(false),
    });
    *slot = Some(Arc::clone(&session));
    let worker_session = Arc::clone(&session);
    if std::thread::Builder::new()
        .name("dd-adapter-ipc".into())
        .spawn(move || {
            let _ = std::panic::catch_unwind(|| serve(&worker_session, sender));
            worker_session.active.store(false, Ordering::Release);
            worker_session.finished.store(true, Ordering::Release);
        })
        .is_err()
    {
        session.active.store(false, Ordering::Release);
        session.finished.store(true, Ordering::Release);
    }
}

fn dispatch(
    session: &Session,
    sender: &SyncSender<Job>,
    request: Request,
    budget: Duration,
    catalog: Option<Arc<dd_catalog::CatalogSnapshot>>,
) -> Response {
    let id = request.id;
    let attempted = uncertain_spawn(&request);
    if budget <= RESPONSE_RESERVE {
        return reject(
            id,
            "Request arrived too close to its transport deadline; nothing was executed.",
        );
    }
    let execution_deadline = Instant::now() + (budget - RESPONSE_RESERVE);
    let schedule_timeout = SCHEDULE_TIMEOUT.min(budget - RESPONSE_RESERVE);
    let state = Arc::new(AtomicU8::new(QUEUED));
    let (response, receiver) = mpsc::sync_channel(1);
    let job = Job {
        request,
        deadline: Instant::now() + schedule_timeout,
        state: Arc::clone(&state),
        response,
        catalog,
    };
    if sender.try_send(job).is_err() {
        return reject(id, "The game thread is busy; request was not executed.");
    }
    // SAFETY: thread was captured in the hook; values contain no cross-process pointers.
    if unsafe {
        PostThreadMessageW(
            session.thread,
            message_id(),
            session.owner_pid as usize,
            WAKE,
        )
    } == 0
        && state
            .compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    {
        return reject(
            id,
            "Could not schedule the request on the game thread; nothing was executed.",
        );
    }
    match receiver.recv_timeout(schedule_timeout) {
        Ok(response) => response,
        Err(_) => {
            match state.compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => reject(
                    id,
                    "Game thread did not accept the request before its deadline; nothing was executed.",
                ),
                Err(RUNNING) => {
                    // A claimed native call cannot be interrupted safely. Package loading may pump
                    // window messages; the queue remains claimed for the entire response budget.
                    receiver.recv_timeout(completion_budget(execution_deadline, Instant::now())).unwrap_or_else(|_| {
                    reject_started(id, "The request started but completion was not confirmed. Check nearby pickups and inventory before retrying.", attempted)
                })
                }
                Err(_) => reject(
                    id,
                    "The queued request expired before execution; nothing was executed.",
                ),
            }
        }
    }
}

fn serve(session: &Session, sender: SyncSender<Job>) -> io::Result<()> {
    let mut pipe = Pipe::bind(std::process::id())?;
    let mut catalogs = crate::catalog_runtime::CatalogState::default();
    while session.alive() {
        pipe.accept_until(|| !session.alive())?;
        let mut stop = false;
        if let Ok(request) = read_frame::<Request>(&mut pipe) {
            pipe.prepare_response(&request);
            stop = request.version == VERSION && matches!(request.command, Command::Shutdown {});
            let response = if !session.alive() {
                reject(
                    request.id,
                    "Adapter attachment ended; request was not executed.",
                )
            } else {
                match catalogs.prepare(&request) {
                    Err(error) => reject(request.id, &error),
                    Ok(prepared) => {
                        let response = dispatch(
                            session,
                            &sender,
                            request,
                            pipe.remaining_time(),
                            prepared.clone(),
                        );
                        if response.result.is_ok() {
                            catalogs.commit(prepared);
                        }
                        response
                    }
                }
            };
            if write_frame(&mut pipe, &response).is_ok() {
                let _ = pipe.read_exact(&mut [0]);
            }
        }
        pipe.disconnect();
        if stop {
            break;
        }
    }
    Ok(())
}

fn on_message(message: &mut MSG) {
    if message_id() == 0 || message.message != message_id() {
        return;
    }
    let owner_pid = message.wParam as u32;
    let operation = message.lParam;
    // Consume only our private registered control messages; preserve the rest of the hook chain.
    message.message = WM_NULL;
    message.wParam = 0;
    message.lParam = 0;
    if operation == START {
        begin(owner_pid);
        return;
    }
    let session = SESSION.try_lock().ok().and_then(|slot| slot.clone());
    let Some(session) = session else { return };
    if owner_pid != session.owner_pid {
        return;
    }
    if operation == STOP {
        session.active.store(false, Ordering::Release);
        return;
    }
    if operation != WAKE || !session.alive() {
        return;
    }
    // SAFETY: query of current thread only; engine work never runs on IPC worker.
    if unsafe { GetCurrentThreadId() } != session.thread {
        return;
    }
    let job = session
        .jobs
        .lock()
        .ok()
        .and_then(|receiver| receiver.try_recv().ok());
    let Some(job) = job else { return };
    if !session.alive() {
        job.state.store(CANCELLED, Ordering::Release);
        let _ = job
            .response
            .try_send(reject(job.request.id, "Request expired before execution."));
        return;
    }
    if !job.begin() {
        return;
    }
    let id = job.request.id;
    let attempted = uncertain_spawn(&job.request);
    let response =
        std::panic::catch_unwind(|| crate::engine::handle(job.request, job.catalog)).unwrap_or_else(|_| {
            reject_started(
                id,
                "Adapter request panicked; completion was not confirmed. Check nearby pickups and inventory before retrying.",
                attempted,
            )
        });
    let _ = job.response.try_send(response);
}

/// Win32 calls this only for the target window's message-retrieval thread.
///
/// # Safety
/// When `code == HC_ACTION`, `lparam` must point to the writable MSG supplied by Windows.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn dd_adapter_hook(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if code == HC_ACTION as i32 && wparam == PM_REMOVE as usize && lparam != 0 {
        let _ = std::panic::catch_unwind(|| {
            // SAFETY: Windows supplies MSG for HC_ACTION; no reference escapes this callback.
            on_message(unsafe { &mut *(lparam as *mut MSG) });
        });
    }
    // SAFETY: the original hook parameters are passed through; hhook is documented as ignored.
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(deadline: Instant) -> Job {
        Job {
            request: Request::new(1, Command::Status {}),
            deadline,
            state: Arc::new(AtomicU8::new(QUEUED)),
            response: mpsc::sync_channel(1).0,
            catalog: None,
        }
    }

    #[test]
    fn timed_out_queued_request_cannot_start_later() {
        let job = job(Instant::now() + SCHEDULE_TIMEOUT);
        assert!(
            job.state
                .compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        );
        assert!(!job.begin());
    }

    #[test]
    fn expired_request_is_rejected_even_if_worker_has_not_cancelled_it_yet() {
        let job = job(Instant::now() - Duration::from_millis(1));
        assert!(!job.begin());
        assert_eq!(job.state.load(Ordering::Acquire), CANCELLED);
    }

    #[test]
    fn executing_request_cannot_be_reported_as_cancelled_before_start() {
        let job = job(Instant::now() + SCHEDULE_TIMEOUT);
        assert!(job.begin());
        assert!(
            job.state
                .compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        );
        assert!(!job.begin());
    }

    #[test]
    fn expired_second_begin_cannot_relabel_running_work_as_cancelled() {
        let mut job = job(Instant::now() + SCHEDULE_TIMEOUT);
        assert!(job.begin());
        job.deadline = Instant::now() - Duration::from_millis(1);
        assert!(!job.begin());
        assert_eq!(job.state.load(Ordering::Acquire), RUNNING);
    }

    #[test]
    fn started_package_load_uses_remaining_response_budget_not_another_queue_timeout() {
        let start = Instant::now();
        let deadline = start + Duration::from_secs(30) - RESPONSE_RESERVE;
        assert_eq!(
            completion_budget(deadline, start + SCHEDULE_TIMEOUT),
            Duration::from_secs(29)
        );
        assert_eq!(
            completion_budget(deadline, start + Duration::from_secs(31)),
            Duration::ZERO
        );
    }

    #[test]
    fn unconfirmed_spawn_preserves_requested_kind_and_uncertainty() {
        for (options, kind) in [
            (SpawnOptions::WorldPickup {}, SpawnKind::WorldPickup),
            (
                SpawnOptions::Collectible { stored_mana: None },
                SpawnKind::EquipmentPickup,
            ),
        ] {
            let request = Request::new(
                4,
                Command::Spawn {
                    catalog: dd_catalog::CatalogKey {
                        installation_id: "0".repeat(64),
                        generation: "1".repeat(64),
                    },
                    template: "Package.Item".into(),
                    quantity: 3,
                    options,
                },
            );
            let response = reject_started(4, "unconfirmed", uncertain_spawn(&request));
            let outcome = response.result.unwrap_err().spawn.unwrap();
            assert_eq!(outcome.requested, 3);
            assert_eq!(outcome.spawned, 0);
            assert_eq!(outcome.kind, kind);
            assert!(outcome.uncertain);
        }
        assert!(uncertain_spawn(&Request::new(5, Command::Status {})).is_none());
        assert!(
            reject(6, "not executed")
                .result
                .unwrap_err()
                .spawn
                .is_none()
        );
    }
}

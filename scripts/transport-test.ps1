param([Parameter(Mandatory=$true)][string]$PackageDirectory)
$ErrorActionPreference = 'Stop'
$hostPath = Join-Path ([IO.Path]::GetFullPath($PackageDirectory)) 'dd-adapter-host.exe'
$adapterProcess = Start-Process -FilePath $hostPath -WindowStyle Hidden -PassThru
$pipeName = 'dungeon-defenders-overlay-' + $adapterProcess.Id

function Connect-TestPipe {
    $pipe = [IO.Pipes.NamedPipeClientStream]::new('.', $pipeName, [IO.Pipes.PipeDirection]::InOut, [IO.Pipes.PipeOptions]::Asynchronous)
    try { $pipe.Connect(4000) } catch { $pipe.Dispose(); throw }
    return $pipe
}

function Read-Exact([IO.Stream]$Stream, [int]$Count) {
    $buffer = [byte[]]::new($Count)
    $offset = 0
    $deadline = [DateTime]::UtcNow.AddSeconds(4)
    while ($offset -lt $Count) {
        $readTask = $Stream.ReadAsync($buffer, $offset, $Count - $offset)
        $remaining = [int][Math]::Max(1, ($deadline - [DateTime]::UtcNow).TotalMilliseconds)
        if (-not $readTask.Wait($remaining)) { throw 'Read timed out.' }
        $readCount = $readTask.Result
        if ($readCount -eq 0) { throw 'Unexpected EOF.' }
        $offset += $readCount
    }
    return ,$buffer
}

function Send-Status([switch]$Fragmented) {
    $pipe = Connect-TestPipe
    try {
        $payload = [Text.Encoding]::UTF8.GetBytes('{"version":6,"id":77,"command":{"type":"status"}}')
        $frame = [BitConverter]::GetBytes([uint32]$payload.Length) + $payload
        if ($Fragmented) {
            foreach ($value in $frame) { $pipe.WriteByte($value); Start-Sleep -Milliseconds 1 }
        } else { $pipe.Write($frame, 0, $frame.Length) }
        $header = Read-Exact $pipe 4
        $length = [BitConverter]::ToUInt32($header, 0)
        if ($length -eq 0 -or $length -gt 16384) { throw 'Invalid response length.' }
        $response = [Text.Encoding]::UTF8.GetString((Read-Exact $pipe $length)) | ConvertFrom-Json
        if ($response.version -ne 6 -or $response.id -ne 77 -or $response.result.Ok.bits -ne 32 -or $response.result.Ok.can_give) { throw 'Invalid status response.' }
        $pipe.WriteByte(1)
    } finally { $pipe.Dispose() }
}

function Assert-PipeClosed([IO.Pipes.NamedPipeClientStream]$Pipe) {
    $buffer = [byte[]]::new(1)
    $readTask = $Pipe.ReadAsync($buffer, 0, 1)
    $closed = $false
    try {
        if (-not $readTask.Wait(3500)) { throw 'Server did not enforce its incomplete-frame deadline.' }
        $closed = $readTask.Result -eq 0
    } catch [AggregateException] {
        # Windows may report ERROR_BROKEN_PIPE rather than EOF on disconnect.
        if ($_.Exception.InnerException -is [IO.IOException]) { $closed = $true } else { throw }
    }
    if (-not $closed) { throw 'Incomplete request did not close.' }
}

function Send-TestRequest([string]$Json) {
    $pipe = Connect-TestPipe
    try {
        $payload = [Text.Encoding]::UTF8.GetBytes($Json)
        $frame = [BitConverter]::GetBytes([uint32]$payload.Length) + $payload
        $pipe.Write($frame, 0, $frame.Length)
        $header = Read-Exact $pipe 4
        $length = [BitConverter]::ToUInt32($header, 0)
        if ($length -eq 0 -or $length -gt 16384) { throw 'Invalid response length.' }
        $response = [Text.Encoding]::UTF8.GetString((Read-Exact $pipe $length)) | ConvertFrom-Json
        if ($response.version -ne 6 -or $response.id -ne 78) { throw 'Response identity mismatch.' }
        $pipe.WriteByte(1)
        return $response
    } finally { $pipe.Dispose() }
}

try {
    Send-Status -Fragmented
    Write-Output 'PASS: fragmented request and framed response through real x86 DLL'

    $pipe = Connect-TestPipe
    try { $header = [BitConverter]::GetBytes([uint32]16385); $pipe.Write($header, 0, $header.Length) } finally { $pipe.Dispose() }
    Send-Status
    Write-Output 'PASS: oversized frame rejected and service remains usable'

    $pipe = Connect-TestPipe
    try {
        $pipe.WriteByte(12)
        # The server must close a client that never finishes its four-byte header.
        Assert-PipeClosed $pipe
    } finally { $pipe.Dispose() }
    Send-Status
    Write-Output 'PASS: incomplete request times out and next client can connect'

    $spawnJson = '{"version":6,"id":78,"command":{"type":"spawn","catalog":{"installation_id":"0000000000000000000000000000000000000000000000000000000000000000","generation":"1111111111111111111111111111111111111111111111111111111111111111"},"template":"Arabia_Weapons.Campaign.BoardswordStyle4_Boss","quantity":1,"options":{"kind":"equipment","options":{"mode":"template_defaults"}}}}'
    $pipe = Connect-TestPipe
    try {
        $payload = [Text.Encoding]::UTF8.GetBytes($spawnJson)
        $header = [BitConverter]::GetBytes([uint32]$payload.Length)
        $pipe.Write($header, 0, $header.Length)
        $pipe.Write($payload, 0, $payload.Length - 1)
        # Seeing a spawn prefix must not extend an incomplete frame to the loading budget.
        Assert-PipeClosed $pipe
    } finally { $pipe.Dispose() }
    Send-Status
    Write-Output 'PASS: incomplete spawn body retains the two-second deadline'

    $response = Send-TestRequest $spawnJson
    if ($response.result.Err.code -ne 'integration_unavailable') { throw 'Diagnostic spawn was not rejected.' }
    Write-Output 'PASS: complete v6 typed spawn reaches diagnostics and remains unavailable'

    $response = Send-TestRequest '{"version":6,"id":78,"command":{"type":"inspect_item","catalog":{"installation_id":"0000000000000000000000000000000000000000000000000000000000000000","generation":"1111111111111111111111111111111111111111111111111111111111111111"},"template":"Arabia_Weapons.Campaign.BoardswordStyle4_Boss"}}'
    if ($response.result.Ok.can_give -or $response.result.Ok.inspection.available -or -not $response.result.Ok.inspection.reason) { throw 'Diagnostic inspection authorized an item or omitted its reason.' }
    Write-Output 'PASS: v6 item inspection reports explicit diagnostic unavailability'

    $response = Send-TestRequest '{"version":6,"id":78,"command":{"type":"bind_catalog","catalog":{"installation_id":"0000000000000000000000000000000000000000000000000000000000000000","generation":"1111111111111111111111111111111111111111111111111111111111111111"}}}'
    if ($response.result.Err.code -ne 'integration_unavailable') { throw 'Diagnostic host bound a game catalog.' }
    Write-Output 'PASS: diagnostic host cannot bind a game catalog'


    $response = Send-TestRequest '{"version":5,"id":78,"command":{"type":"status"}}'
    if ($response.result.Err.code -ne 'protocol_mismatch') { throw 'Version-five request was not rejected.' }
    Send-Status
    Write-Output 'PASS: legacy version rejected and service remains usable'

} finally {
    if (-not $adapterProcess.HasExited) { $adapterProcess.Kill(); $adapterProcess.WaitForExit() }
    $adapterProcess.Dispose()
}

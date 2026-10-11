//! Fixed testing conditions applied only to a freshly constructed equipment instance.
use super::{Engine, Result, read, require, u32_at, word};
use windows_sys::Win32::System::{
    Diagnostics::Debug::WriteProcessMemory, Threading::GetCurrentProcess,
};

const EQUIPMENT: &str = "UDKGame.HeroEquipment";

fn write(address: u32, bytes: &[u8]) -> Result<()> {
    read(address, bytes.len())?;
    let mut written = 0;
    // SAFETY: callers validate reflected fields or a private, bounded candidate-owned array.
    // The OS performs a checked write into this process; no game globals or templates are written.
    let ok = unsafe {
        WriteProcessMemory(
            GetCurrentProcess(),
            address as usize as _,
            bytes.as_ptr().cast(),
            bytes.len(),
            &mut written,
        )
    };
    require(
        ok != 0 && written == bytes.len(),
        "Could not set temporary loot conditions.",
    )
}

pub(super) fn prepare(engine: &Engine, template: u32, item: u32) -> Result<()> {
    engine.check_thread()?;
    engine.is_a(item, EQUIPMENT)?;
    require(
        item != template && u32_at(item + 0x38)? == template && u32_at(item + 8)? & 0x400 == 0,
        "Loot conditions require a fresh equipment instance.",
    )?;
    // Both branches of DetermineUltQuality use the normal template probabilities.
    // This removes the Ruthless-mode dependency without changing the GRI or forcing a tier.
    for (suffix, expected_offset) in [
        ("Ultimate93Chance", 2168),
        ("UltimatePlusChance", 2172),
        ("UltimatePlusPlusChance", 2176),
    ] {
        let source =
            engine.reflect_field(&format!("{EQUIPMENT}.{suffix}"), "Core.FloatProperty", 4)?;
        let target = engine.reflect_field(
            &format!("{EQUIPMENT}.Ruthless{suffix}"),
            "Core.FloatProperty",
            4,
        )?;
        require(
            source.count == 1
                && target.count == 1
                && source.offset == expected_offset
                && target.offset == expected_offset + 12,
            "Promotion chance layout changed.",
        )?;
        let value = read(template + source.offset, 4)?;
        let chance = f32::from_le_bytes(value.as_slice().try_into().expect("float"));
        require(
            chance.is_finite() && (0.0..=1.0).contains(&chance),
            "Invalid template promotion chance.",
        )?;
        write(item + source.offset, &value)?;
        write(item + target.offset, &value)?;
    }

    let path = format!("{EQUIPMENT}.MaxLevelRangeDifficultyArray");
    let field = engine.reflect_field(&path, "Core.ArrayProperty", 12)?;
    let inner = u32_at(engine.reference(&path)? + 0x84)?;
    require(
        field.count == 1
            && field.offset == 2684
            && u32_at(inner + 0x34)? == engine.reference("Core.StructProperty")?
            && u32_at(inner + 0x48)? == 8
            && u32_at(inner + 0x84)? == engine.reference("UDKGame.HeroEquipment.LevelRange")?,
        "Upgrade range array layout changed.",
    )?;
    for (name, offset) in [("MinLevel", 0), ("MaxLevel", 4)] {
        let field = engine.reflect_field(
            &format!("{EQUIPMENT}.LevelRange.{name}"),
            "Core.IntProperty",
            4,
        )?;
        require(
            field.offset == offset && field.count == 1,
            "Upgrade range layout changed.",
        )?;
    }
    let original = read(template + field.offset, 12)?;
    let candidate = read(item + field.offset, 12)?;
    let count = word(&original, 4);
    require(
        count <= word(&original, 8)
            && word(&original, 8) <= 16
            && word(&candidate, 4) == count
            && count <= word(&candidate, 8)
            && word(&candidate, 8) <= 16,
        "Unsupported upgrade range array.",
    )?;
    if count > 0 {
        let length = count * 8;
        let source = word(&original, 0);
        let target = word(&candidate, 0);
        require(
            disjoint(
                source,
                word(&original, 8) * 8,
                target,
                word(&candidate, 8) * 8,
            ),
            "Candidate upgrade ranges share template storage; no roll was performed.",
        )?;
        let ranges = read(source, length as usize)?;
        let normalized = fixed_ranges(&ranges)?;
        // UE constructed a separate array. Preserve its pointer, count and allocator ownership.
        write(target, &normalized)?;
    }
    Ok(())
}

fn disjoint(a: u32, a_len: u32, b: u32, b_len: u32) -> bool {
    a >= 0x10000
        && b >= 0x10000
        && a.checked_add(a_len)
            .zip(b.checked_add(b_len))
            .is_some_and(|(a_end, b_end)| a_end <= b || b_end <= a)
}

fn fixed_ranges(ranges: &[u8]) -> Result<Vec<u8>> {
    require(
        !ranges.is_empty() && ranges.len().is_multiple_of(8),
        "Invalid upgrade ranges.",
    )?;
    let highest = &ranges[ranges.len() - 8..];
    let min = word(highest, 0) as i32;
    let max = word(highest, 4) as i32;
    require(
        min >= 1 && max >= min && max <= 9999,
        "Unsupported highest-difficulty upgrade range.",
    )?;
    Ok(highest.repeat(ranges.len() / 8))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_difficulty_uses_the_templates_last_upgrade_range() {
        let values: Vec<u8> = [1i32, 10, 20, 50, 100, 250]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .collect();
        let result = fixed_ranges(&values).unwrap();
        for range in result.as_chunks::<8>().0 {
            assert_eq!((word(range, 0), word(range, 4)), (100, 250));
        }
        assert_eq!(word(&values, 0), 1);
    }
    #[test]
    fn aliases_and_overflow_cannot_be_written() {
        assert!(!disjoint(0x10000, 40, 0x10010, 40));
        assert!(!disjoint(u32::MAX - 3, 8, 0x10000, 8));
        assert!(disjoint(0x10000, 40, 0x11000, 40));
    }
}

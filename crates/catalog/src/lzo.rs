//! Bounded LZO1X decoding for the verified UE3 compression blocks.
//! No encoder or general archive handling is needed by the package reader.

pub(crate) fn decompress(input: &[u8], expected: usize) -> Result<Vec<u8>, String> {
    let mut decoder = Decoder {
        input,
        position: 0,
        output: Vec::with_capacity(expected),
        expected,
    };
    let first = *input.first().ok_or("Empty LZO block")?;
    let mut token;
    let mut first_literal = false;
    if first > 17 {
        decoder.position += 1;
        let count = usize::from(first - 17);
        decoder.literals(count)?;
        token = decoder.byte()?;
        first_literal = count >= 4;
    } else {
        token = decoder.byte()?;
        if token < 16 {
            let count = decoder.length(usize::from(token), 15)? + 3;
            decoder.literals(count)?;
            token = decoder.byte()?;
            first_literal = true;
        }
    }
    loop {
        if first_literal && token < 16 {
            let distance =
                1 + 0x800 + usize::from(token >> 2) + (usize::from(decoder.byte()?) << 2);
            decoder.copy_match(distance, 3)?;
        } else if token >= 64 {
            let distance = 1 + usize::from((token >> 2) & 7) + (usize::from(decoder.byte()?) << 3);
            decoder.copy_match(distance, usize::from(token >> 5) + 1)?;
        } else if token >= 32 {
            let length = decoder.length(usize::from(token & 31), 31)? + 2;
            let distance = 1 + (usize::from(decoder.word()?) >> 2);
            decoder.copy_match(distance, length)?;
        } else if token >= 16 {
            let length = decoder.length(usize::from(token & 7), 7)? + 2;
            let distance = (usize::from(token & 8) << 11) + (usize::from(decoder.word()?) >> 2);
            if distance == 0 {
                if length != 3
                    || decoder.position != input.len()
                    || decoder.output.len() != expected
                {
                    return Err("Invalid LZO end marker or decoded length".into());
                }
                return Ok(decoder.output);
            }
            decoder.copy_match(distance + 0x4000, length)?;
        } else {
            let distance = 1 + usize::from(token >> 2) + (usize::from(decoder.byte()?) << 2);
            decoder.copy_match(distance, 2)?;
        }
        first_literal = false;
        let trailing = usize::from(input[decoder.position - 2] & 3);
        if trailing != 0 {
            decoder.literals(trailing)?;
            token = decoder.byte()?;
        } else {
            token = decoder.byte()?;
            if token < 16 {
                let length = decoder.length(usize::from(token), 15)? + 3;
                decoder.literals(length)?;
                token = decoder.byte()?;
                first_literal = true;
            }
        }
    }
}

struct Decoder<'a> {
    input: &'a [u8],
    position: usize,
    output: Vec<u8>,
    expected: usize,
}

impl Decoder<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let result = *self.input.get(self.position).ok_or("Truncated LZO block")?;
        self.position += 1;
        Ok(result)
    }

    fn word(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes([self.byte()?, self.byte()?]))
    }

    fn length(&mut self, initial: usize, base: usize) -> Result<usize, String> {
        if initial != 0 {
            return Ok(initial);
        }
        let mut length = base;
        loop {
            let byte = self.byte()?;
            length = length
                .checked_add(if byte == 0 { 255 } else { usize::from(byte) })
                .filter(|value| *value <= self.expected)
                .ok_or("Oversized LZO length")?;
            if byte != 0 {
                return Ok(length);
            }
        }
    }

    fn reserve(&self, count: usize) -> Result<(), String> {
        if count > self.expected.saturating_sub(self.output.len()) {
            return Err("LZO output exceeds declared block length".into());
        }
        Ok(())
    }

    fn literals(&mut self, count: usize) -> Result<(), String> {
        self.reserve(count)?;
        let end = self
            .position
            .checked_add(count)
            .ok_or("LZO literal overflow")?;
        self.output.extend_from_slice(
            self.input
                .get(self.position..end)
                .ok_or("Truncated LZO literals")?,
        );
        self.position = end;
        Ok(())
    }

    fn copy_match(&mut self, distance: usize, count: usize) -> Result<(), String> {
        self.reserve(count)?;
        if distance == 0 || distance > self.output.len() {
            return Err("LZO match references unavailable output".into());
        }
        for _ in 0..count {
            self.output.push(self.output[self.output.len() - distance]);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::decompress;

    #[test]
    fn literal_and_overlapping_match() {
        assert_eq!(
            decompress(&[22, b'h', b'e', b'l', b'l', b'o', 17, 0, 0], 5).unwrap(),
            b"hello"
        );
        assert_eq!(
            decompress(&[18, b'x', 64, 0, 17, 0, 0], 4).unwrap(),
            b"xxxx"
        );
    }

    #[test]
    fn malformed_blocks_are_rejected() {
        for block in [
            &[][..],
            &[22, b'h'][..],
            &[64, 0, 17, 0, 0][..],
            &[17, 0, 0, 7][..],
        ] {
            assert!(decompress(block, 5).is_err());
        }
        assert!(decompress(&[22, b'h', b'e', b'l', b'l', b'o', 17, 0, 0], 4).is_err());
    }
}

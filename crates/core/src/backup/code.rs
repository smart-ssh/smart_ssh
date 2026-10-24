//! Recovery code (Spec 0109, K3).
//!
//! 28 random characters of a 32-symbol alphabet (140 bits from the OS RNG)
//! plus two check characters, shown in six groups of five. The alphabet has
//! no `I`, `L`, `O`, `U`; on input `O` reads as `0`, `I`/`L` as `1`. Case,
//! spaces and dashes are ignored. The check characters are two sums mod 32
//! (plain and position-weighted), so **every** single-character change and
//! every swap of two adjacent different characters is detected.

use std::fmt;

use zeroize::Zeroizing;

use super::{random_bytes, BackupError};

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const DATA_CHARS: usize = 28;
const CODE_CHARS: usize = DATA_CHARS + 2;
const GROUP: usize = 5;

/// A recovery code. Zeroized on drop; `Debug` never shows it.
pub struct RecoveryCode(Zeroizing<String>);

impl RecoveryCode {
    /// A fresh code from the OS random generator.
    pub fn generate() -> Result<RecoveryCode, BackupError> {
        let random = random_bytes::<DATA_CHARS>()?;
        let mut values: Zeroizing<Vec<u8>> = Zeroizing::new(Vec::with_capacity(CODE_CHARS));
        values.extend(random.iter().map(|b| b & 0x1f));
        let (s1, s2) = checksum(&values);
        values.push(s1);
        values.push(s2);
        let text: String = values
            .iter()
            .map(|&v| ALPHABET[v as usize] as char)
            .collect();
        Ok(RecoveryCode(Zeroizing::new(text)))
    }

    /// Parses user input. A wrong length, a character outside the alphabet or
    /// a checksum mismatch is [`BackupError::CodeTypo`] (K3).
    pub fn parse(input: &str) -> Result<RecoveryCode, BackupError> {
        let mut values: Zeroizing<Vec<u8>> = Zeroizing::new(Vec::with_capacity(CODE_CHARS));
        for ch in input.chars() {
            if ch.is_whitespace() || ch == '-' {
                continue;
            }
            let up = ch.to_ascii_uppercase();
            let up = match up {
                'O' => '0',
                'I' | 'L' => '1',
                c => c,
            };
            let Some(pos) = ALPHABET.iter().position(|&a| a as char == up) else {
                return Err(BackupError::CodeTypo);
            };
            if values.len() == CODE_CHARS {
                return Err(BackupError::CodeTypo);
            }
            values.push(pos as u8);
        }
        if values.len() != CODE_CHARS {
            return Err(BackupError::CodeTypo);
        }
        let (s1, s2) = checksum(&values[..DATA_CHARS]);
        if values[DATA_CHARS] != s1 || values[DATA_CHARS + 1] != s2 {
            return Err(BackupError::CodeTypo);
        }
        let text: String = values
            .iter()
            .map(|&v| ALPHABET[v as usize] as char)
            .collect();
        Ok(RecoveryCode(Zeroizing::new(text)))
    }

    /// The code for display or writing down, e.g. `ABCDE-FGHJK-…`. The result
    /// is zeroized on drop; only the UI layer should ever call this, once.
    pub fn display_string(&self) -> Zeroizing<String> {
        let mut out = Zeroizing::new(String::with_capacity(CODE_CHARS + CODE_CHARS / GROUP));
        for (i, ch) in self.0.chars().enumerate() {
            if i > 0 && i % GROUP == 0 {
                out.push('-');
            }
            out.push(ch);
        }
        out
    }

    /// Canonical form (upper case, no separators) – the KDF input.
    pub(crate) fn canonical(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for RecoveryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryCode(<redacted>)")
    }
}

fn checksum(values: &[u8]) -> (u8, u8) {
    let mut s1: u32 = 0;
    let mut s2: u32 = 0;
    for (i, &v) in values.iter().enumerate() {
        s1 += u32::from(v);
        s2 += (i as u32 + 1) * u32::from(v);
    }
    ((s1 % 32) as u8, (s2 % 32) as u8)
}

#[cfg(test)]
pub(crate) fn alphabet() -> &'static [u8; 32] {
    ALPHABET
}

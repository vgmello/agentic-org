use super::Error;
use ring::rand::{SecureRandom, SystemRandom};
use zeroize::Zeroizing;
pub struct Code(pub(super) Zeroizing<[u8; 20]>);
impl std::fmt::Debug for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Code(<redacted>)")
    }
}
impl Code {
    pub fn generate() -> Result<Self, Error> {
        let mut code = Zeroizing::new([0; 20]);
        SystemRandom::new()
            .fill(&mut *code)
            .map_err(|_| Error::Unavailable)?;
        Ok(Self(code))
    }
    pub fn parse(input: &str) -> Result<Self, Error> {
        if input.len() > 128 {
            return Err(Error::InvalidCode);
        }
        let mut code = Zeroizing::new([0; 20]);
        let mut digits = 0;
        for b in input.bytes() {
            if b == b'-' || b.is_ascii_whitespace() {
                continue;
            }
            let n = match b {
                b'0'..=b'9' => b - b'0',
                b'A'..=b'F' => b - b'A' + 10,
                b'a'..=b'f' => b - b'a' + 10,
                _ => return Err(Error::InvalidCode),
            };
            if digits >= 40 {
                return Err(Error::InvalidCode);
            }
            code[digits / 2] |= n << if digits % 2 == 0 { 4 } else { 0 };
            digits += 1;
        }
        if digits != 40 {
            return Err(Error::InvalidCode);
        }
        Ok(Self(code))
    }
    pub fn formatted(&self) -> Zeroizing<String> {
        let mut out = Zeroizing::new(String::with_capacity(49));
        use std::fmt::Write;
        for (index, b) in self.0.iter().enumerate() {
            if index > 0 && index % 2 == 0 {
                out.push('-');
            }
            write!(out, "{b:02X}").expect("String write");
        }
        out
    }
}

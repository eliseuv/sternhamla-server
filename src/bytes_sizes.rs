use std::fmt::Display;

const KILO: usize = 1024;
const MEGA: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub enum ByteSize {
    Byte(usize),
    KiloByte(f64),
    MegaByte(f64),
}

impl From<usize> for ByteSize {
    fn from(value: usize) -> Self {
        if value / KILO == 0 {
            Self::Byte(value)
        } else if value / MEGA == 0 {
            Self::KiloByte(value as f64 / KILO as f64)
        } else {
            Self::MegaByte(value as f64 / MEGA as f64)
        }
    }
}

impl Display for ByteSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ByteSize::Byte(size_b) => write!(f, "{size_b}B"),
            ByteSize::KiloByte(size_kb) => write!(f, "{size_kb}KB"),
            ByteSize::MegaByte(size_mb) => write!(f, "{size_mb}MB"),
        }
    }
}

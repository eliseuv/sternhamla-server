use std::fmt::Display;

const KILO: usize = 1024;
const MEGA: usize = 1024 * 1024;
const GIGA: usize = 1024 * 1024 * 1024;
const TERA: usize = 1024 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub enum ByteSize {
    Byte(usize),
    KiloByte(f64),
    MegaByte(f64),
    GigaByte(f64),
    TeraByte(f64),
}

impl From<usize> for ByteSize {
    fn from(value: usize) -> Self {
        if value / KILO == 0 {
            Self::Byte(value)
        } else if value / MEGA == 0 {
            Self::KiloByte(value as f64 / KILO as f64)
        } else if value / GIGA == 0 {
            Self::MegaByte(value as f64 / MEGA as f64)
        } else if value / TERA == 0 {
            Self::GigaByte(value as f64 / GIGA as f64)
        } else {
            Self::TeraByte(value as f64 / TERA as f64)
        }
    }
}

impl Display for ByteSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ByteSize::Byte(size_b) => write!(f, "{size_b} B"),
            ByteSize::KiloByte(size_kb) => write!(f, "{size_kb} KB"),
            ByteSize::MegaByte(size_mb) => write!(f, "{size_mb} MB"),
            ByteSize::GigaByte(size_gb) => write!(f, "{size_gb} GB"),
            ByteSize::TeraByte(size_tb) => write!(f, "{size_tb} TB"),
        }
    }
}

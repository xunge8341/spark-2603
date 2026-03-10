/// Interest flags (READ/WRITE) without extra dependencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interest(u8);

impl Default for Interest {
    #[inline]
    fn default() -> Self {
        Self::empty()
    }
}


impl Interest {
    pub const READ: Self = Self(0b01);
    pub const WRITE: Self = Self(0b10);

    /// Empty interest set.
    ///
    /// Backends may treat this as an error or map it to a safe default.
    #[inline]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub const fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }

    #[inline]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

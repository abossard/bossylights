//! Checked light counts and runs, shared by the patch and the tick.

/// The most lights an engine can have: 64 universes of 170 RGB pixels and more.
pub const MAX_LIGHTS: u16 = 16_384;

/// How many lights this engine has, fixed while the engine runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lights(u16);

impl Lights {
    /// [`MAX_LIGHTS`].
    pub const MOST: Self = Self(MAX_LIGHTS);

    /// `None` outside 1 to [`MAX_LIGHTS`].
    pub const fn new(count: u16) -> Option<Self> {
        if count >= 1 && count <= MAX_LIGHTS {
            Some(Self(count))
        } else {
            None
        }
    }

    pub const fn count(self) -> u16 {
        self.0
    }
}

/// One light of this engine, counted from 0, below its [`Lights`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightNumber(u16);

impl LightNumber {
    /// `None` at or past `lights`.
    pub const fn new(index: u16, lights: Lights) -> Option<Self> {
        if index < lights.0 {
            Some(Self(index))
        } else {
            None
        }
    }

    pub const fn index(self) -> u16 {
        self.0
    }
}

/// Lights one after the other, from `first`. Checked against [`Lights`] when made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LightRun {
    first: u16,
    count: u16,
}

impl LightRun {
    /// `None` when it is empty or runs past `lights`.
    pub fn new(first: LightNumber, count: u16, lights: Lights) -> Option<Self> {
        let end = first.0.checked_add(count)?;
        (count > 0 && end <= lights.0).then_some(Self {
            first: first.0,
            count,
        })
    }

    pub const fn first(self) -> LightNumber {
        LightNumber(self.first)
    }

    pub const fn count(self) -> u16 {
        self.count
    }

    /// These lights of `lights`, the values by number. Fewer when `lights` is shorter.
    pub fn of<T>(self, lights: &mut [T]) -> &mut [T] {
        let first = usize::from(self.first).min(lights.len());
        let end = (first + usize::from(self.count)).min(lights.len());
        lights.get_mut(first..end).unwrap_or_default()
    }
}

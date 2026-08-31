//! Read-only domain model and capability boundary for Mazda infotainment data.
//!
//! Intentionally absent: raw transports, arbitrary IPC, CAN/LIN writes, VIP access,
//! shell execution, and filesystem mutation.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeedKph(f32);

impl SpeedKph {
    #[must_use]
    pub fn new(value: f32) -> Self {
        Self(value.max(0.0))
    }

    #[must_use]
    pub fn get(self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TemperatureC(f32);

impl TemperatureC {
    #[must_use]
    pub const fn new(value: f32) -> Self {
        Self(value)
    }

    #[must_use]
    pub const fn get(self) -> f32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gear {
    Park,
    Reverse,
    Neutral,
    Drive,
    Manual,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VehicleSnapshot {
    pub speed: SpeedKph,
    pub gear: Gear,
    pub outside_temperature: TemperatureC,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Paused,
    Playing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaState {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub playback: PlaybackState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommanderEvent {
    RotateClockwise,
    RotateCounterClockwise,
    Select,
    Back,
    Home,
    Music,
    Navigation,
    Favorites,
}

/// The only Mazda platform capability ordinary application code receives.
///
/// Implementations may talk to a simulator, a recording, or eventually a CMU,
/// but callers cannot access the underlying transport.
pub trait MazdaReadOnly {
    fn vehicle_snapshot(&self) -> VehicleSnapshot;
    fn media_state(&self) -> MediaState;
    fn next_commander_event(&mut self) -> Option<CommanderEvent>;
}

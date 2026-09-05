//! Read-only domain model and capability boundary for Mazda infotainment data.
//!
//! Intentionally absent: raw transports, arbitrary IPC, CAN/LIN writes, VIP access,
//! shell execution, and filesystem mutation.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpeedKph(Option<f32>);

impl SpeedKph {
    /// Creates a speed reading, preserving invalid input as unavailable data.
    ///
    /// Negative and non-finite values are not valid vehicle speeds. They must not be
    /// normalized to zero because zero is a valid reading with a different meaning.
    #[must_use]
    pub fn new(value: f32) -> Self {
        if value.is_finite() && value >= 0.0 {
            Self(Some(value))
        } else {
            Self::unavailable()
        }
    }

    #[must_use]
    pub const fn unavailable() -> Self {
        Self(None)
    }

    #[must_use]
    pub const fn get(self) -> Option<f32> {
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

#[cfg(test)]
mod tests {
    use super::SpeedKph;

    #[test]
    fn speed_preserves_valid_zero_and_positive_values() {
        assert_eq!(SpeedKph::new(0.0).get(), Some(0.0));
        assert_eq!(SpeedKph::new(42.5).get(), Some(42.5));
    }

    #[test]
    fn invalid_speed_is_explicitly_unavailable() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.1] {
            assert_eq!(SpeedKph::new(value), SpeedKph::unavailable());
            assert_eq!(SpeedKph::new(value).get(), None);
        }
    }
}

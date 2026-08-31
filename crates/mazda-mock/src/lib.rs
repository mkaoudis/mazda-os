use std::collections::VecDeque;

use mazda_core::{
    CommanderEvent, Gear, MazdaReadOnly, MediaState, PlaybackState, SpeedKph, TemperatureC,
    VehicleSnapshot,
};

#[derive(Debug, Clone)]
pub struct MockMazda {
    vehicle: VehicleSnapshot,
    media: MediaState,
    commander_events: VecDeque<CommanderEvent>,
}

impl MockMazda {
    #[must_use]
    pub fn demo() -> Self {
        Self {
            vehicle: VehicleSnapshot {
                speed: SpeedKph::new(0.0),
                gear: Gear::Park,
                outside_temperature: TemperatureC::new(22.0),
            },
            media: MediaState {
                title: Some("Everything in Its Right Place".to_owned()),
                artist: Some("Radiohead".to_owned()),
                album: Some("Kid A".to_owned()),
                playback: PlaybackState::Playing,
            },
            commander_events: [
                CommanderEvent::RotateClockwise,
                CommanderEvent::RotateClockwise,
                CommanderEvent::Select,
                CommanderEvent::Home,
            ]
            .into(),
        }
    }

    /// Mutation exists only on the mock fixture so tests and simulators can create scenarios.
    pub fn set_vehicle_snapshot(&mut self, vehicle: VehicleSnapshot) {
        self.vehicle = vehicle;
    }

    /// Mutation exists only on the mock fixture so tests and simulators can create scenarios.
    pub fn set_media_state(&mut self, media: MediaState) {
        self.media = media;
    }

    pub fn push_commander_event(&mut self, event: CommanderEvent) {
        self.commander_events.push_back(event);
    }
}

impl MazdaReadOnly for MockMazda {
    fn vehicle_snapshot(&self) -> VehicleSnapshot {
        self.vehicle.clone()
    }

    fn media_state(&self) -> MediaState {
        self.media.clone()
    }

    fn next_commander_event(&mut self) -> Option<CommanderEvent> {
        self.commander_events.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_scenario_is_stationary_and_playing_media() {
        let mazda = MockMazda::demo();
        let vehicle = mazda.vehicle_snapshot();
        let media = mazda.media_state();

        assert_eq!(vehicle.gear, Gear::Park);
        assert_eq!(vehicle.speed.get(), 0.0);
        assert_eq!(media.playback, PlaybackState::Playing);
    }

    #[test]
    fn commander_events_are_deterministic() {
        let mut mazda = MockMazda::demo();

        assert_eq!(
            mazda.next_commander_event(),
            Some(CommanderEvent::RotateClockwise)
        );
        assert_eq!(
            mazda.next_commander_event(),
            Some(CommanderEvent::RotateClockwise)
        );
        assert_eq!(mazda.next_commander_event(), Some(CommanderEvent::Select));
        assert_eq!(mazda.next_commander_event(), Some(CommanderEvent::Home));
        assert_eq!(mazda.next_commander_event(), None);
    }
}

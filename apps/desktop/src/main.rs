use mazda_core::MazdaReadOnly;
use mazda_mock::MockMazda;

fn main() {
    let mut mazda = MockMazda::demo();

    println!("vehicle: {:?}", mazda.vehicle_snapshot());
    println!("media: {:?}", mazda.media_state());

    while let Some(event) = mazda.next_commander_event() {
        println!("commander: {event:?}");
    }
}

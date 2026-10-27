//! Example: drive the FusionEngine with synthetic sensor data.

use mediamaster_core::traits::SensorReading;
use mediamaster_sensors::{DebouncedSensor, FusionEngine};
use std::time::Duration;

fn main() {
    let mut engine = FusionEngine::new();

    // No readings → empty.
    println!("initial:  {:?}", engine.current_state());

    // Low-light environment.
    engine.update("light", SensorReading::Light(10.0));
    println!("light 10: {:?}", engine.current_state());

    // Motion trumps low light regardless of insertion order.
    engine.update("pir", SensorReading::MotionDetected);
    println!("+ motion: {:?}", engine.current_state());

    // Clear motion, still low light.
    engine.update("pir", SensorReading::Light(10.0));
    println!("motion gone, {:?}", engine.current_state());

    // Demonstrate the debouncer with a zero window.
    let mut db = DebouncedSensor::new(Duration::from_millis(0));
    match db.update(SensorReading::Open) {
        None => println!("debouncer: None (first call — starts window)"),
        Some(r) => println!("debouncer: {:?}", r),
    }
    match db.update(SensorReading::Open) {
        None => println!("debouncer: None (still in window)"),
        Some(r) => println!("debouncer: {:?}", r),
    }

    // Show the priority ordering explicitly.
    let mut engine2 = FusionEngine::new();
    engine2.update("far", SensorReading::Proximity(20.0));
    engine2.update("dim", SensorReading::Light(30.0));
    println!("\npriority demo:");
    println!(
        "  proximity 20cm + light 30lux → {:?}",
        engine2.current_state()
    );

    engine2.update("near", SensorReading::Proximity(5.0));
    println!("  + proximity 5cm         → {:?}", engine2.current_state());

    engine2.update("motion", SensorReading::MotionDetected);
    println!("  + motion                → {:?}", engine2.current_state());
}

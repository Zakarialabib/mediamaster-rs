use mediamaster_core::{EventBus, HardwareEvent};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type ZoneId = String;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub id: ZoneId,
    pub name: String,
    pub output_device_id: Option<String>,
    pub volume_db: f32,
    pub muted: bool,
    pub sync_group: Option<String>,
}

pub trait ZoneRouter {
    fn list_zones(&self) -> Vec<Zone>;
    fn zone_by_name(&self, name: &str) -> Option<Zone>;
    fn assign_output(&mut self, zone_id: &ZoneId, device_id: Option<String>) -> Result<(), String>;
    fn set_zone_volume(&mut self, zone_id: &ZoneId, volume_db: f32) -> Result<(), String>;
    fn set_zone_mute(&mut self, zone_id: &ZoneId, muted: bool) -> Result<(), String>;
    fn set_sync_group(
        &mut self,
        zone_id: &ZoneId,
        sync_group: Option<String>,
    ) -> Result<(), String>;
}

pub struct Zones<'a> {
    zones: HashMap<ZoneId, Zone>,
    event_bus: &'a EventBus,
}

impl<'a> Zones<'a> {
    pub fn new(event_bus: &'a EventBus) -> Self {
        Self {
            zones: HashMap::new(),
            event_bus,
        }
    }

    pub fn add_zone(&mut self, zone: Zone) {
        self.zones.insert(zone.id.clone(), zone);
    }

    pub fn delete_zone(&mut self, zone_id: &ZoneId) -> Option<Zone> {
        self.zones.remove(zone_id)
    }

    fn update_group_volume_or_mute(
        &mut self,
        sync_group: Option<&String>,
        volume_db: Option<f32>,
        muted: Option<bool>,
        zone_id: &ZoneId,
    ) -> Result<(), String> {
        let mut affected_zones = Vec::new();

        if let Some(group) = sync_group {
            for zone in self.zones.values_mut() {
                if zone.sync_group.as_ref() == Some(group) {
                    if let Some(v) = volume_db {
                        zone.volume_db = v;
                    }
                    if let Some(m) = muted {
                        zone.muted = m;
                    }
                    affected_zones.push(zone.id.clone());
                }
            }
        } else {
            if let Some(zone) = self.zones.get_mut(zone_id) {
                if let Some(v) = volume_db {
                    zone.volume_db = v;
                }
                if let Some(m) = muted {
                    zone.muted = m;
                }
                affected_zones.push(zone.id.clone());
            } else {
                return Err("Zone not found".to_string());
            }
        }

        if !affected_zones.is_empty() {
            let sample_zone = self.zones.get(&affected_zones[0]).unwrap();
            let event_vol = sample_zone.volume_db;
            let event_muted = sample_zone.muted;

            let _ = self.event_bus.publish(HardwareEvent::ZoneChanged {
                zone_ids: affected_zones,
                volume_db: event_vol,
                muted: event_muted,
            });
        }

        Ok(())
    }
}

impl<'a> ZoneRouter for Zones<'a> {
    fn list_zones(&self) -> Vec<Zone> {
        self.zones.values().cloned().collect()
    }

    fn zone_by_name(&self, name: &str) -> Option<Zone> {
        self.zones.values().find(|z| z.name == name).cloned()
    }

    fn assign_output(&mut self, zone_id: &ZoneId, device_id: Option<String>) -> Result<(), String> {
        if let Some(zone) = self.zones.get_mut(zone_id) {
            zone.output_device_id = device_id;
            Ok(())
        } else {
            Err("Zone not found".to_string())
        }
    }

    fn set_zone_volume(&mut self, zone_id: &ZoneId, volume_db: f32) -> Result<(), String> {
        let sync_group = self.zones.get(zone_id).and_then(|z| z.sync_group.clone());
        let sg_ref = sync_group.as_ref();

        // Ensure zone exists before updating
        if !self.zones.contains_key(zone_id) {
            return Err("Zone not found".to_string());
        }

        self.update_group_volume_or_mute(sg_ref, Some(volume_db), None, zone_id)
    }

    fn set_zone_mute(&mut self, zone_id: &ZoneId, muted: bool) -> Result<(), String> {
        let sync_group = self.zones.get(zone_id).and_then(|z| z.sync_group.clone());
        let sg_ref = sync_group.as_ref();

        if !self.zones.contains_key(zone_id) {
            return Err("Zone not found".to_string());
        }

        self.update_group_volume_or_mute(sg_ref, None, Some(muted), zone_id)
    }

    fn set_sync_group(
        &mut self,
        zone_id: &ZoneId,
        sync_group: Option<String>,
    ) -> Result<(), String> {
        if let Some(zone) = self.zones.get_mut(zone_id) {
            zone.sync_group = sync_group;
            Ok(())
        } else {
            Err("Zone not found".to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mediamaster_core::EventBus;

    #[test]
    fn test_zone_crud_and_lookup() {
        let bus = EventBus::new(10);
        let mut router = Zones::new(&bus);

        let z1 = Zone {
            id: "z1".to_string(),
            name: "Lobby".to_string(),
            output_device_id: None,
            volume_db: 0.0,
            muted: false,
            sync_group: None,
        };

        router.add_zone(z1.clone());
        assert_eq!(router.list_zones().len(), 1);

        let found = router.zone_by_name("Lobby");
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "z1");

        router
            .assign_output(&"z1".to_string(), Some("dev1".to_string()))
            .unwrap();
        assert_eq!(
            router.zone_by_name("Lobby").unwrap().output_device_id,
            Some("dev1".to_string())
        );

        router.delete_zone(&"z1".to_string());
        assert_eq!(router.list_zones().len(), 0);
    }

    #[test]
    fn test_group_atomic_volume_apply() {
        let bus = EventBus::new(10);
        let mut rx = bus.subscribe();
        let mut router = Zones::new(&bus);

        let z1 = Zone {
            id: "z1".to_string(),
            name: "Lobby".to_string(),
            output_device_id: None,
            volume_db: 0.0,
            muted: false,
            sync_group: Some("group1".to_string()),
        };
        let z2 = Zone {
            id: "z2".to_string(),
            name: "Lobby2".to_string(),
            output_device_id: None,
            volume_db: 0.0,
            muted: false,
            sync_group: Some("group1".to_string()),
        };

        router.add_zone(z1);
        router.add_zone(z2);

        router.set_zone_volume(&"z1".to_string(), -10.0).unwrap();

        let z1_updated = router.zones.get("z1").unwrap();
        let z2_updated = router.zones.get("z2").unwrap();

        assert_eq!(z1_updated.volume_db, -10.0);
        assert_eq!(z2_updated.volume_db, -10.0);

        if let Ok(event) = rx.try_recv() {
            if let HardwareEvent::ZoneChanged {
                zone_ids,
                volume_db,
                muted,
            } = event
            {
                assert_eq!(zone_ids.len(), 2);
                assert!(zone_ids.contains(&"z1".to_string()));
                assert!(zone_ids.contains(&"z2".to_string()));
                assert_eq!(volume_db, -10.0);
                assert!(!muted);
            } else {
                panic!("Wrong event type");
            }
        } else {
            panic!("Event not received");
        }
    }

    #[test]
    fn test_serialization_round_trip() {
        let z1 = Zone {
            id: "z1".to_string(),
            name: "Lobby".to_string(),
            output_device_id: Some("dev_a".to_string()),
            volume_db: -5.5,
            muted: true,
            sync_group: Some("group_a".to_string()),
        };

        let serialized = serde_json::to_string(&z1).unwrap();
        let deserialized: Zone = serde_json::from_str(&serialized).unwrap();

        assert_eq!(z1, deserialized);
    }
}

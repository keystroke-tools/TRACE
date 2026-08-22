//! Private little-endian readers for documented vanilla AC page prefixes.

use std::collections::BTreeMap;

use crate::AcPageError;

pub(crate) const PHYSICS_PREFIX_LENGTH: usize = 200;
pub(crate) const GRAPHICS_PREFIX_LENGTH: usize = 264;
pub(crate) const STATIC_PREFIX_LENGTH: usize = 476;
pub(crate) const PHYSICS_PAGE_LENGTH: usize = 580;
pub(crate) const GRAPHICS_PAGE_LENGTH: usize = 296;
pub(crate) const STATIC_PAGE_LENGTH: usize = 684;
const STATIC_SHARED_MEMORY_VERSION_OFFSET: usize = 0;
const STATIC_AC_VERSION_OFFSET: usize = 30;
const STATIC_VERSION_SLOTS: usize = 15;
const STATIC_CAR_MODEL_OFFSET: usize = 68;
const STATIC_TRACK_OFFSET: usize = 134;
const STATIC_TRACK_CONFIGURATION_OFFSET: usize = 524;
const STATIC_ID_SLOTS: usize = 33;
const STATIC_AIR_TEMPERATURE_OFFSET: usize = 456;
const STATIC_ROAD_TEMPERATURE_OFFSET: usize = 460;

pub(crate) struct PhysicsPage<'a>(&'a [u8]);

impl<'a> PhysicsPage<'a> {
    pub(crate) fn parse(bytes: &'a [u8]) -> Result<Self, AcPageError> {
        require_length(bytes, PHYSICS_PREFIX_LENGTH).map(Self)
    }

    pub(crate) fn gas(&self) -> f32 {
        read_f32(self.0, 4)
    }
    pub(crate) fn packet_id(&self) -> i32 {
        read_i32(self.0, 0)
    }
    pub(crate) fn brake(&self) -> f32 {
        read_f32(self.0, 8)
    }
    pub(crate) fn fuel(&self) -> f32 {
        read_f32(self.0, 12)
    }
    pub(crate) fn gear(&self) -> i32 {
        read_i32(self.0, 16)
    }
    pub(crate) fn rpm(&self) -> i32 {
        read_i32(self.0, 20)
    }
    pub(crate) fn speed_kmh(&self) -> f32 {
        read_f32(self.0, 28)
    }
    pub(crate) fn velocity(&self) -> [f32; 3] {
        read_f32_array(self.0, 32)
    }
    pub(crate) fn acceleration_g(&self) -> [f32; 3] {
        read_f32_array(self.0, 44)
    }
    pub(crate) fn tyre_core_temperature(&self, index: usize) -> f32 {
        read_f32(self.0, 152 + index * 4)
    }
    pub(crate) fn suspension_travel(&self, index: usize) -> f32 {
        read_f32(self.0, 184 + index * 4)
    }
    pub(crate) fn number_of_tyres_out(&self) -> Option<i32> {
        self.0
            .get(244..248)
            .and_then(|bytes| <[u8; 4]>::try_from(bytes).ok())
            .map(i32::from_le_bytes)
    }
}

pub(crate) struct GraphicsPage<'a>(&'a [u8]);

impl<'a> GraphicsPage<'a> {
    pub(crate) fn parse(bytes: &'a [u8]) -> Result<Self, AcPageError> {
        require_length(bytes, GRAPHICS_PREFIX_LENGTH).map(Self)
    }

    pub(crate) fn status(&self) -> i32 {
        read_i32(self.0, 4)
    }

    pub(crate) fn session_type(&self) -> i32 {
        read_i32(self.0, 8)
    }

    pub(crate) fn packet_id(&self) -> i32 {
        read_i32(self.0, 0)
    }

    pub(crate) fn completed_laps(&self) -> i32 {
        read_i32(self.0, 132)
    }
    pub(crate) fn current_time_ms(&self) -> i32 {
        read_i32(self.0, 140)
    }
    pub(crate) fn current_sector_index(&self) -> i32 {
        read_i32(self.0, 164)
    }
    pub(crate) fn last_sector_time_ms(&self) -> i32 {
        read_i32(self.0, 168)
    }
    pub(crate) fn normalized_car_position(&self) -> f32 {
        read_f32(self.0, 248)
    }
    pub(crate) fn car_coordinates(&self) -> [f32; 3] {
        read_f32_array(self.0, 252)
    }
}

pub(crate) struct StaticPage<'a>(&'a [u8]);

impl<'a> StaticPage<'a> {
    pub(crate) fn parse(bytes: &'a [u8]) -> Result<Self, AcPageError> {
        require_length(bytes, STATIC_PREFIX_LENGTH).map(Self)
    }

    pub(crate) fn car_model(&self) -> Option<String> {
        read_utf16(self.0, STATIC_CAR_MODEL_OFFSET, STATIC_ID_SLOTS)
    }
    pub(crate) fn track(&self) -> Option<String> {
        read_utf16(self.0, STATIC_TRACK_OFFSET, STATIC_ID_SLOTS)
    }
    pub(crate) fn track_configuration(&self) -> Option<String> {
        if self.0.len() < STATIC_TRACK_CONFIGURATION_OFFSET + STATIC_ID_SLOTS * 2 {
            return None;
        }
        read_utf16(self.0, STATIC_TRACK_CONFIGURATION_OFFSET, STATIC_ID_SLOTS)
    }
    pub(crate) fn air_temperature(&self) -> f32 {
        read_f32(self.0, STATIC_AIR_TEMPERATURE_OFFSET)
    }
    pub(crate) fn road_temperature(&self) -> f32 {
        read_f32(self.0, STATIC_ROAD_TEMPERATURE_OFFSET)
    }
    pub(crate) fn shared_memory_version(&self) -> Option<String> {
        read_utf16(
            self.0,
            STATIC_SHARED_MEMORY_VERSION_OFFSET,
            STATIC_VERSION_SLOTS,
        )
    }
    pub(crate) fn assetto_corsa_version(&self) -> Option<String> {
        read_utf16(self.0, STATIC_AC_VERSION_OFFSET, STATIC_VERSION_SLOTS)
    }
}

#[allow(clippy::too_many_lines)]
pub(crate) fn native_float_fields(
    physics: &[u8],
    graphics: &[u8],
    static_page: &[u8],
) -> BTreeMap<String, f64> {
    let mut fields = BTreeMap::new();
    insert_f32_fields(
        &mut fields,
        physics,
        "physics",
        &[
            (4, "gas"),
            (8, "brake"),
            (12, "fuel_litres"),
            (24, "steer_angle"),
            (28, "speed_kmh"),
            (200, "drs"),
            (204, "tc"),
            (208, "heading"),
            (212, "pitch"),
            (216, "roll"),
            (220, "cg_height"),
            (252, "abs"),
            (256, "kers_charge"),
            (260, "kers_input"),
            (276, "turbo_boost"),
            (280, "ballast_kg"),
            (284, "air_density"),
            (288, "air_temperature_c"),
            (292, "road_temperature_c"),
            (308, "final_ff"),
            (312, "performance_meter"),
            (336, "kers_current_kj"),
            (364, "clutch"),
            (564, "brake_bias"),
        ],
    );
    for (offset, name, count) in [
        (32, "velocity", 3),
        (44, "acceleration_g", 3),
        (56, "wheel_slip", 4),
        (72, "wheel_load_n", 4),
        (88, "wheel_pressure", 4),
        (104, "wheel_angular_speed", 4),
        (120, "tyre_wear", 4),
        (136, "tyre_dirty_level", 4),
        (152, "tyre_core_temperature_c", 4),
        (168, "camber_rad", 4),
        (184, "suspension_travel_m", 4),
        (224, "car_damage", 5),
        (268, "ride_height", 2),
        (296, "local_angular_velocity", 3),
        (348, "brake_temperature_c", 4),
        (368, "tyre_temperature_inner_c", 4),
        (384, "tyre_temperature_middle_c", 4),
        (400, "tyre_temperature_outer_c", 4),
        (420, "tyre_contact_point", 12),
        (468, "tyre_contact_normal", 12),
        (516, "tyre_contact_heading", 12),
        (568, "local_velocity", 3),
    ] {
        insert_f32_array(&mut fields, physics, "physics", name, offset, count);
    }
    insert_f32_fields(
        &mut fields,
        graphics,
        "graphics",
        &[
            (152, "session_time_left_s"),
            (156, "distance_traveled_m"),
            (244, "replay_time_multiplier"),
            (248, "normalized_car_position"),
            (264, "penalty_time_s"),
            (280, "surface_grip"),
            (288, "wind_speed"),
            (292, "wind_direction_degrees"),
        ],
    );
    insert_f32_array(&mut fields, graphics, "graphics", "car_coordinates", 252, 3);
    insert_f32_fields(
        &mut fields,
        static_page,
        "static",
        &[
            (404, "max_torque"),
            (408, "max_power"),
            (416, "max_fuel_litres"),
            (452, "max_turbo_boost"),
            (456, "deprecated_1"),
            (460, "deprecated_2"),
            (468, "aid_fuel_rate"),
            (472, "aid_tyre_rate"),
            (476, "aid_mechanical_damage"),
            (484, "aid_stability"),
            (508, "kers_max_j"),
            (520, "track_spline_length_m"),
            (592, "ers_max_j"),
        ],
    );
    insert_f32_array(
        &mut fields,
        static_page,
        "static",
        "suspension_max_travel",
        420,
        4,
    );
    insert_f32_array(&mut fields, static_page, "static", "tyre_radius", 436, 4);
    fields
}

pub(crate) fn native_integer_fields(
    physics: &[u8],
    graphics: &[u8],
    static_page: &[u8],
) -> BTreeMap<String, i64> {
    let mut fields = BTreeMap::new();
    insert_i32_fields(
        &mut fields,
        physics,
        "physics",
        &[
            (0, "packet_id"),
            (16, "gear"),
            (20, "rpm"),
            (244, "number_of_tyres_out"),
            (248, "pit_limiter_on"),
            (264, "auto_shifter_on"),
            (316, "engine_brake"),
            (320, "ers_recovery_level"),
            (324, "ers_power_level"),
            (328, "ers_heat_charging"),
            (332, "ers_is_charging"),
            (340, "drs_available"),
            (344, "drs_enabled"),
            (416, "is_ai_controlled"),
        ],
    );
    insert_i32_fields(
        &mut fields,
        graphics,
        "graphics",
        &[
            (0, "packet_id"),
            (4, "status"),
            (8, "session"),
            (132, "completed_laps"),
            (136, "position"),
            (140, "current_time_ms"),
            (144, "last_time_ms"),
            (148, "best_time_ms"),
            (160, "is_in_pit"),
            (164, "current_sector_index"),
            (168, "last_sector_time_ms"),
            (172, "number_of_laps"),
            (268, "flag"),
            (272, "ideal_line_on"),
            (276, "is_in_pit_lane"),
            (284, "mandatory_pit_done"),
        ],
    );
    insert_i32_fields(
        &mut fields,
        static_page,
        "static",
        &[
            (60, "number_of_sessions"),
            (64, "number_of_cars"),
            (400, "sector_count"),
            (412, "max_rpm"),
            (464, "penalties_enabled"),
            (480, "aid_allow_tyre_blankets"),
            (488, "aid_auto_clutch"),
            (492, "aid_auto_blip"),
            (496, "has_drs"),
            (500, "has_ers"),
            (504, "has_kers"),
            (512, "engine_brake_settings_count"),
            (516, "ers_power_controller_count"),
            (596, "is_timed_race"),
            (600, "has_extra_lap"),
            (672, "reversed_grid_positions"),
            (676, "pit_window_start"),
            (680, "pit_window_end"),
        ],
    );
    fields
}

pub(crate) fn native_text_fields(graphics: &[u8], static_page: &[u8]) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for (offset, slots, name) in [
        (12, 15, "graphics.current_time"),
        (42, 15, "graphics.last_time"),
        (72, 15, "graphics.best_time"),
        (102, 15, "graphics.split"),
        (176, 33, "graphics.tyre_compound"),
    ] {
        if graphics.len() >= offset + slots * 2
            && let Some(value) = read_utf16(graphics, offset, slots)
        {
            fields.insert(name.into(), value);
        }
    }
    for (offset, slots, name) in [
        (0, 15, "static.shared_memory_version"),
        (30, 15, "static.ac_version"),
        (68, 33, "static.car_model"),
        (134, 33, "static.track"),
        (200, 33, "static.player_name"),
        (266, 33, "static.player_surname"),
        (332, 33, "static.player_nick"),
        (524, 33, "static.track_configuration"),
        (604, 33, "static.car_skin"),
    ] {
        if static_page.len() >= offset + slots * 2
            && let Some(value) = read_utf16(static_page, offset, slots)
        {
            fields.insert(name.into(), value);
        }
    }
    fields
}

fn insert_f32_fields(
    fields: &mut BTreeMap<String, f64>,
    bytes: &[u8],
    page: &str,
    definitions: &[(usize, &str)],
) {
    for &(offset, name) in definitions {
        if bytes.len() >= offset + 4 {
            fields.insert(format!("{page}.{name}"), f64::from(read_f32(bytes, offset)));
        }
    }
}

fn insert_i32_fields(
    fields: &mut BTreeMap<String, i64>,
    bytes: &[u8],
    page: &str,
    definitions: &[(usize, &str)],
) {
    for &(offset, name) in definitions {
        if bytes.len() >= offset + 4 {
            fields.insert(format!("{page}.{name}"), i64::from(read_i32(bytes, offset)));
        }
    }
}

fn insert_f32_array(
    fields: &mut BTreeMap<String, f64>,
    bytes: &[u8],
    page: &str,
    name: &str,
    offset: usize,
    count: usize,
) {
    for index in 0..count {
        let item_offset = offset + index * 4;
        if bytes.len() >= item_offset + 4 {
            fields.insert(
                format!("{page}.{name}.{index}"),
                f64::from(read_f32(bytes, item_offset)),
            );
        }
    }
}

pub(crate) fn redacted_static_page(
    shared_memory_version: Option<&str>,
    assetto_corsa_version: Option<&str>,
    car_model: Option<&str>,
    track: Option<&str>,
    air_temperature: Option<f32>,
    road_temperature: Option<f32>,
) -> Vec<u8> {
    let mut bytes = vec![0; STATIC_PREFIX_LENGTH];
    write_utf16(
        &mut bytes,
        STATIC_SHARED_MEMORY_VERSION_OFFSET,
        STATIC_VERSION_SLOTS,
        shared_memory_version,
    );
    write_utf16(
        &mut bytes,
        STATIC_AC_VERSION_OFFSET,
        STATIC_VERSION_SLOTS,
        assetto_corsa_version,
    );
    write_utf16(
        &mut bytes,
        STATIC_CAR_MODEL_OFFSET,
        STATIC_ID_SLOTS,
        car_model,
    );
    write_utf16(&mut bytes, STATIC_TRACK_OFFSET, STATIC_ID_SLOTS, track);
    if let Some(value) = air_temperature {
        bytes[STATIC_AIR_TEMPERATURE_OFFSET..STATIC_AIR_TEMPERATURE_OFFSET + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    if let Some(value) = road_temperature {
        bytes[STATIC_ROAD_TEMPERATURE_OFFSET..STATIC_ROAD_TEMPERATURE_OFFSET + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

fn require_length(bytes: &[u8], expected: usize) -> Result<&[u8], AcPageError> {
    if bytes.len() < expected {
        Err(AcPageError::TooShort {
            expected,
            actual: bytes.len(),
        })
    } else {
        Ok(bytes)
    }
}

fn read_i32(bytes: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated page prefix"),
    )
}

fn read_f32(bytes: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("validated page prefix"),
    )
}

fn read_f32_array(bytes: &[u8], offset: usize) -> [f32; 3] {
    [
        read_f32(bytes, offset),
        read_f32(bytes, offset + 4),
        read_f32(bytes, offset + 8),
    ]
}

fn read_utf16(bytes: &[u8], offset: usize, slots: usize) -> Option<String> {
    let units: Vec<u16> = (0..slots)
        .map(|index| {
            let start = offset + index * 2;
            u16::from_le_bytes([bytes[start], bytes[start + 1]])
        })
        .take_while(|unit| *unit != 0)
        .collect();
    if units.is_empty() {
        None
    } else {
        String::from_utf16(&units)
            .ok()
            .filter(|value| !value.is_empty())
    }
}

fn write_utf16(bytes: &mut [u8], offset: usize, slots: usize, value: Option<&str>) {
    let Some(value) = value else {
        return;
    };
    for (index, unit) in value
        .encode_utf16()
        .take(slots.saturating_sub(1))
        .enumerate()
    {
        let start = offset + index * 2;
        bytes[start..start + 2].copy_from_slice(&unit.to_le_bytes());
    }
}

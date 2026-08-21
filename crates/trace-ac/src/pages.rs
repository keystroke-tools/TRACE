//! Private little-endian readers for documented vanilla AC page prefixes.

use crate::AcPageError;

pub(crate) const PHYSICS_PREFIX_LENGTH: usize = 200;
pub(crate) const GRAPHICS_PREFIX_LENGTH: usize = 292;
pub(crate) const STATIC_PREFIX_LENGTH: usize = 476;

pub(crate) struct PhysicsPage<'a>(&'a [u8]);

impl<'a> PhysicsPage<'a> {
    pub(crate) fn parse(bytes: &'a [u8]) -> Result<Self, AcPageError> {
        require_length(bytes, PHYSICS_PREFIX_LENGTH).map(Self)
    }

    pub(crate) fn gas(&self) -> f32 {
        read_f32(self.0, 4)
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
}

pub(crate) struct GraphicsPage<'a>(&'a [u8]);

impl<'a> GraphicsPage<'a> {
    pub(crate) fn parse(bytes: &'a [u8]) -> Result<Self, AcPageError> {
        require_length(bytes, GRAPHICS_PREFIX_LENGTH).map(Self)
    }

    pub(crate) fn completed_laps(&self) -> i32 {
        read_i32(self.0, 140)
    }
    pub(crate) fn current_time_ms(&self) -> i32 {
        read_i32(self.0, 148)
    }
    pub(crate) fn normalized_car_position(&self) -> f32 {
        read_f32(self.0, 256)
    }
    pub(crate) fn car_coordinates(&self) -> [f32; 3] {
        read_f32_array(self.0, 260)
    }
}

pub(crate) struct StaticPage<'a>(&'a [u8]);

impl<'a> StaticPage<'a> {
    pub(crate) fn parse(bytes: &'a [u8]) -> Result<Self, AcPageError> {
        require_length(bytes, STATIC_PREFIX_LENGTH).map(Self)
    }

    pub(crate) fn car_model(&self) -> Option<String> {
        read_utf16(self.0, 72, 33)
    }
    pub(crate) fn track(&self) -> Option<String> {
        read_utf16(self.0, 140, 33)
    }
    pub(crate) fn air_temperature(&self) -> f32 {
        read_f32(self.0, 468)
    }
    pub(crate) fn road_temperature(&self) -> f32 {
        read_f32(self.0, 472)
    }
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

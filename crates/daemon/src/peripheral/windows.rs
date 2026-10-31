#![allow(dead_code)]

/// FICTIVE WINDOWS REALIZATION
use crate::peripheral::PhysicalState;
use anyhow::Result;
use msla_core::types::peripheral::StepperPositioning;

#[derive(Clone)]
pub struct PeripheralController {}

impl PeripheralController {
    /// Creates new peripheral controller
    pub async fn new() -> Result<Self> {
        let s = Self {};
        s.init().await?;
        Ok(s)
    }

    /// Initialize/start code
    async fn init(&self) -> Result<()> {
        Ok(())
    }

    /// Home Z axis
    pub async fn home_z(&self, _offset: f64) -> Result<()> {
        Ok(())
    }

    /// Moves Z axis to a provided pos and with provided speed
    pub async fn move_z_to(
        &self,
        _pos: f64,
        _speed: f64,
        _positioning: StepperPositioning,
    ) -> Result<()> {
        Ok(())
    }

    /// Turn UV backlight
    pub async fn turn_uv(&self, _state: bool) -> Result<()> {
        Ok(())
    }

    /// Sets a motor current and returns a current current
    pub async fn set_motor_current(&self, _current: u16) -> Result<u16> {
        Ok(500)
    }

    /// Disable stepper (driver EN pin)
    pub async fn disable_steppers(&self) -> Result<()> {
        Ok(())
    }

    /// Enable stepper (driver EN pin)
    pub async fn enable_steppers(&self) -> Result<()> {
        Ok(())
    }

    /// Get stepper pos (mm)
    pub async fn get_physical_state(&self) -> Result<PhysicalState> {
        Ok(PhysicalState {
            uv_state: false,
            z_pos: 0.0,
        })
    }

    /// Set system fan speed
    pub async fn set_sys_fan_speed(&self, _speed: u8) -> Result<()> {
        Ok(())
    }

    pub async fn reset_client(&self) {}
}

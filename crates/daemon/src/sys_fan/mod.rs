use std::time::Duration;

use anyhow::Result;
use tokio::time::sleep;
use tracing::error;

use crate::{peripheral::PeripheralController, sys_fan::therm::get_temp};
mod therm;

/// Start sys fan
pub async fn start_fan(peripheral_controller: PeripheralController) -> Result<()> {
    #[cfg(target_os = "linux")]
    loop {
        if let Err(e) = fan_loop(&peripheral_controller).await {
            error!("System fan error: {}", e);
            sleep(Duration::from_secs(5)).await;
        }
    }

    #[cfg(not(target_os = "linux"))]
    Ok(())
}

#[allow(unused)]
async fn fan_loop(peripheral_controller: &PeripheralController) -> Result<()> {
    loop {
        let temp = get_temp().await?;

        let pwm_duty = ((temp as f64 - 40.0) / 20.0).clamp(0.0, 1.0);
        peripheral_controller
            .set_sys_fan_speed((pwm_duty * 255.0).round() as u8)
            .await?;

        sleep(Duration::from_secs(3)).await;
    }
}

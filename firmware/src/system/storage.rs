use crate::fader::calibration::CalibrationResult;
use embassy_rp::flash::{Async, Flash};
use embassy_rp::peripherals::FLASH;
use embedded_storage_async::nor_flash::NorFlash;

// Linker symbols for our dedicated storage section.
unsafe extern "C" {
    static __storage_start: u32;
}

/// We reserve a 4KB sector for persistent storage.
/// Note: RP2350/RP2040 flash drivers require 4-byte alignment for DMA buffers.
pub const FLASH_SIZE: usize = 2 * 1024 * 1024;
const MAGIC: u32 = 0x4448_4431; // "DHD1" in hex (little endian)
const VERSION: u32 = 1;

/// The storage is placed in a dedicated section defined in the linker script.
/// We initialize it with the magic and version so the firmware recognizes it
/// as a valid (but empty) storage area immediately after flashing.
#[unsafe(link_section = ".storage")]
#[used]
static STORAGE_INIT: [u8; 16] = [
    0x31, 0x44, 0x48, 0x44, // Magic "DHD1"
    0x01, 0x00, 0x00, 0x00, // Version 1
    0x00, 0x00, 0x00, 0x00, // Checksum (0 for empty)
    0x00, 0x00, 0x00, 0x00, // Data length (0 for empty)
];

/// A 4-byte aligned buffer for DMA-safe flash operations.
#[repr(C, align(4))]
struct AlignedBuffer<const N: usize>([u8; N]);

/// Shared buffer for flash operations.
/// Since all flash activity happens in the main orchestrator loop, we can
/// safely use a single static buffer to avoid stack overflows and alignment
/// issues.
static mut FLASH_BUF: AlignedBuffer<4096> = AlignedBuffer([0; 4096]);

/// Simple CRC-32 (IEEE) implementation for no_std.
fn calculate_crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

pub async fn load_calibration(
    flash: &mut Flash<'_, FLASH, Async, FLASH_SIZE>,
) -> Option<CalibrationResult> {
    let storage_addr = unsafe { &__storage_start as *const u32 as u32 };
    let offset = storage_addr - 0x1000_0000;

    // Safety: we are in the main loop, no concurrent access to FLASH_BUF.
    // We use addr_of_mut! to satisfy Rust 2024 safety requirements.
    let buf = unsafe { &mut *core::ptr::addr_of_mut!(FLASH_BUF.0) };

    log::debug!("Loading calibration from address 0x{:x}", storage_addr);

    // Read the header (first 16 bytes)
    if let Err(e) = flash.read(offset, &mut buf[..16]).await {
        log::error!("Flash read error at 0x{:x}: {:?}", offset, e);
        return None;
    }

    let magic = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    let version = u32::from_le_bytes(buf[4..8].try_into().unwrap());
    let stored_checksum = u32::from_le_bytes(buf[8..12].try_into().unwrap());
    let len = u32::from_le_bytes(buf[12..16].try_into().unwrap());

    if magic != MAGIC {
        log::warn!("Invalid storage magic: 0x{:08x}", magic);
        return None;
    }

    if version != VERSION {
        log::warn!("Storage version mismatch: {}", version);
        return None;
    }

    if len == 0 {
        log::info!("Storage is initialized but empty (newly flashed)");
        return None;
    }

    if len > 512 {
        log::warn!("Invalid storage length: {}", len);
        return None;
    }

    // Read the actual data. We MUST read a multiple of 4 bytes for the RP2350
    // DMA-based flash driver to accept the transfer.
    let read_len = (len as usize + 3) & !3;
    if let Err(e) = flash.read(offset + 16, &mut buf[16..16 + read_len]).await {
        log::error!("Flash data read error: {:?}", e);
        return None;
    }

    let data = &buf[16..16 + len as usize];
    let calculated_checksum = calculate_crc32(data);
    if calculated_checksum != stored_checksum {
        log::error!(
            "Storage checksum mismatch! (Stored: 0x{:08x}, Calculated: 0x{:08x})",
            stored_checksum,
            calculated_checksum
        );
        return None;
    }

    match serde_json_core::from_slice::<CalibrationResult>(data) {
        Ok((res, _)) => {
            log::info!("Calibration verified and loaded ({} bytes)", len);
            Some(res)
        }
        Err(e) => {
            log::error!("Failed to parse verified calibration JSON: {:?}", e);
            None
        }
    }
}

pub async fn store_calibration(
    flash: &mut Flash<'_, FLASH, Async, FLASH_SIZE>,
    cal: &CalibrationResult,
) {
    let storage_addr = unsafe { &__storage_start as *const u32 as u32 };
    let offset = storage_addr - 0x1000_0000;

    // Safety: we are in the main loop, no concurrent access to FLASH_BUF.
    // We use addr_of_mut! to satisfy Rust 2024 safety requirements.
    let buf = unsafe { &mut *core::ptr::addr_of_mut!(FLASH_BUF.0) };

    // Clear buffer to ensure a clean slate
    buf.fill(0);

    match serde_json_core::to_slice(cal, &mut buf[16..]) {
        Ok(len) => {
            let checksum = calculate_crc32(&buf[16..16 + len]);

            buf[0..4].copy_from_slice(&MAGIC.to_le_bytes());
            buf[4..8].copy_from_slice(&VERSION.to_le_bytes());
            buf[8..12].copy_from_slice(&checksum.to_le_bytes());
            buf[12..16].copy_from_slice(&(len as u32).to_le_bytes());

            log::debug!(
                "Persisting calibration to 0x{:x} ({} bytes, CRC: 0x{:08x})",
                storage_addr,
                len,
                checksum
            );

            if let Err(e) = flash.erase(offset, offset + 4096).await {
                log::error!("Flash erase failed: {:?}", e);
                return;
            }

            if let Err(e) = flash.write(offset, buf).await {
                log::error!("Flash write failed: {:?}", e);
            } else {
                log::info!("Calibration successfully committed to permanent storage.");
            }
        }
        Err(e) => {
            log::error!("Failed to serialize calibration for storage: {:?}", e);
        }
    }
}

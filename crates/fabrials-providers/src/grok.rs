use fabrials_core::{ResetCredit, ResetInventory};

pub const RESET_URL: &str = "https://grok.com/prod_mc_billing.ConsumerUiSvc/GetRemainingResets";

/// Validate every HTTP header or gRPC-web trailer status before exposing inventory.
pub fn validate_reset_status(status: &str) -> Result<(), &'static str> {
    if status.trim() == "0" {
        Ok(())
    } else {
        Err("reset inventory rejected")
    }
}

/// ConsumerUiSvc: response.tokens=10, token.validity_start=20, token.validity_end=30.
/// Inventory only; redemption identifiers are never returned.
pub fn parse_resets(bytes: &[u8], now_ms: i64) -> Result<ResetInventory, &'static str> {
    if bytes.len() > 2 * 1024 * 1024 {
        return Err("reset response too large");
    }
    let mut remaining = bytes;
    let mut credits = Vec::new();
    let mut available: u32 = 0;
    let mut saw_frame = false;
    while !remaining.is_empty() {
        if remaining.len() < 5 {
            return Err("truncated grpc frame");
        }
        let flag = remaining[0];
        let size = u32::from_be_bytes(
            remaining[1..5]
                .try_into()
                .map_err(|_| "invalid grpc frame")?,
        ) as usize;
        remaining = &remaining[5..];
        let frame = remaining.get(..size).ok_or("truncated grpc payload")?;
        remaining = &remaining[size..];
        if flag == 0x80 {
            let trailer = std::str::from_utf8(frame).map_err(|_| "invalid grpc trailer")?;
            for line in trailer.lines() {
                if let Some((name, value)) = line.split_once(':') {
                    if name.trim().eq_ignore_ascii_case("grpc-status") {
                        validate_reset_status(value)?;
                    }
                }
            }
        } else if flag == 0 {
            saw_frame = true;
            fields(frame, |number, data| {
                if number != 10 {
                    return Ok(());
                }
                let mut credit = ResetCredit {
                    valid_from_ms: None,
                    expires_at_ms: None,
                };
                fields(data, |field, time| {
                    if field == 20 {
                        credit.valid_from_ms = Some(timestamp(time)?);
                    }
                    if field == 30 {
                        credit.expires_at_ms = Some(timestamp(time)?);
                    }
                    Ok(())
                })?;
                if credit.valid_from_ms.is_some_and(|start| start > now_ms)
                    || credit.expires_at_ms.is_some_and(|end| end <= now_ms)
                {
                    return Ok(());
                }
                available = available.checked_add(1).ok_or("too many resets")?;
                let expiry = credit.expires_at_ms.unwrap_or(i64::MAX);
                let position = credits.partition_point(|existing: &ResetCredit| {
                    existing.expires_at_ms.unwrap_or(i64::MAX) <= expiry
                });
                if position < 64 {
                    credits.insert(position, credit);
                    credits.truncate(64);
                }
                Ok(())
            })?;
        } else {
            return Err("unsupported grpc frame");
        }
    }
    if !saw_frame {
        return Err("missing reset data");
    }
    Ok(ResetInventory {
        available,
        details_complete: available as usize == credits.len(),
        credits,
    })
}

fn varint(bytes: &mut &[u8]) -> Result<u64, &'static str> {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let byte = *bytes.first().ok_or("truncated protobuf integer")?;
        *bytes = &bytes[1..];
        if shift == 63 && byte > 1 {
            return Err("protobuf integer overflow");
        }
        value |= u64::from(byte & 127) << shift;
        if byte & 128 == 0 {
            return Ok(value);
        }
    }
    Err("invalid protobuf integer")
}

fn fields(
    mut bytes: &[u8],
    mut visit: impl FnMut(u64, &[u8]) -> Result<(), &'static str>,
) -> Result<(), &'static str> {
    while !bytes.is_empty() {
        let key = varint(&mut bytes)?;
        if key >> 3 == 0 {
            return Err("invalid protobuf field");
        }
        let size = match key & 7 {
            0 => {
                varint(&mut bytes)?;
                continue;
            }
            1 => 8,
            2 => usize::try_from(varint(&mut bytes)?).map_err(|_| "protobuf size overflow")?,
            5 => 4,
            _ => return Err("unsupported protobuf wire type"),
        };
        let data = bytes.get(..size).ok_or("truncated protobuf field")?;
        bytes = &bytes[size..];
        if key & 7 == 2 {
            visit(key >> 3, data)?;
        }
    }
    Ok(())
}

fn timestamp(mut bytes: &[u8]) -> Result<i64, &'static str> {
    let mut seconds = 0u64;
    let mut nanos = 0u64;
    while !bytes.is_empty() {
        let key = varint(&mut bytes)?;
        if key & 7 != 0 {
            return Err("invalid timestamp");
        }
        let value = varint(&mut bytes)?;
        if key >> 3 == 1 {
            seconds = value;
        }
        if key >> 3 == 2 {
            if value >= 1_000_000_000 {
                return Err("invalid timestamp nanos");
            }
            nanos = value;
        }
    }
    i64::try_from(seconds)
        .ok()
        .and_then(|v| v.checked_mul(1000))
        .and_then(|v| v.checked_add((nanos / 1_000_000) as i64))
        .ok_or("timestamp overflow")
}

pub mod device;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_inventory_keeps_earliest_expiries_regardless_of_wire_order() {
        let mut payload = [82, 0].repeat(64);
        for seconds in (1..=100).rev() {
            payload.extend([82, 5, 242, 1, 2, 8, seconds]);
        }
        let mut frame = vec![0];
        frame.extend((payload.len() as u32).to_be_bytes());
        frame.extend(payload);
        let inventory = parse_resets(&frame, 0).unwrap();
        assert_eq!(inventory.available, 164);
        assert!(!inventory.details_complete);
        assert_eq!(inventory.credits.len(), 64);
        let expiries: Vec<_> = inventory
            .credits
            .iter()
            .map(|credit| credit.expires_at_ms)
            .collect();
        assert_eq!(
            expiries,
            (1..=64)
                .map(|seconds| Some(seconds * 1000))
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn fractional_validity_boundaries_preserve_available_resets() {
        fn integer(mut value: u64) -> Vec<u8> {
            let mut bytes = Vec::new();
            while value >= 128 {
                bytes.push((value as u8 & 127) | 128);
                value >>= 7;
            }
            bytes.push(value as u8);
            bytes
        }
        fn message(field: u64, data: &[u8]) -> Vec<u8> {
            let mut bytes = integer((field << 3) | 2);
            bytes.extend(integer(data.len() as u64));
            bytes.extend(data);
            bytes
        }
        let mut time = vec![8];
        time.extend(integer(1000));
        time.push(16);
        time.extend(integer(500_000_000));
        assert_eq!(timestamp(&time).unwrap(), 1_000_500);
        for field in [20, 30] {
            let payload = message(10, &message(field, &time));
            let mut frame = vec![0];
            frame.extend((payload.len() as u32).to_be_bytes());
            frame.extend(payload);
            let before = parse_resets(&frame, 1_000_499).unwrap();
            let at = parse_resets(&frame, 1_000_500).unwrap();
            assert_eq!(before.available, u32::from(field == 30));
            assert_eq!(at.available, u32::from(field == 20));
        }
        time = vec![16];
        time.extend(integer(1_000_000_000));
        assert_eq!(timestamp(&time), Err("invalid timestamp nanos"));
    }
    #[test]
    fn rejects_transport_success_with_grpc_failure() {
        let mut data = vec![0, 0, 0, 0, 0];
        let trailer = b"grpc-status: 16\r\n";
        data.push(128);
        data.extend_from_slice(&(trailer.len() as u32).to_be_bytes());
        data.extend_from_slice(trailer);
        assert!(parse_resets(&data, 0).is_err());
        assert!(parse_resets(&[], 0).is_err());
        assert!(parse_resets(&[0, 0, 0, 0, 2, 1], 0).is_err());
    }
    #[test]
    fn rejects_failed_or_malformed_header_status() {
        assert!(validate_reset_status("0").is_ok());
        assert!(validate_reset_status(" 0 ").is_ok());
        for status in ["16", "", "0, 16", "00", "unavailable"] {
            assert!(validate_reset_status(status).is_err());
        }
    }
    #[test]
    fn empty_valid_inventory_is_zero() {
        assert_eq!(parse_resets(&[0, 0, 0, 0, 0], 0).unwrap().available, 0);
    }
}

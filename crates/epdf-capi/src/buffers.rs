use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use epdf_engine::host::{HostResponse, Payloads};

use crate::abi::Status;

pub(crate) struct ByteBudget {
    used: AtomicU64,
    limit: u64,
}

pub(crate) struct Reservation {
    budget: Arc<ByteBudget>,
    bytes: u64,
}

impl ByteBudget {
    pub(crate) fn new(limit: u64) -> Arc<Self> {
        Arc::new(Self { used: AtomicU64::new(0), limit })
    }

    pub(crate) fn reserve(self: &Arc<Self>, bytes: u64) -> Result<Reservation, Status> {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|next| *next <= self.limit)
            })
            .map_err(|_| Status::LimitExceeded)?;
        Ok(Reservation { budget: Arc::clone(self), bytes })
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}

fn packed_len(response: &HostResponse, limit: usize) -> Result<usize, Status> {
    let json = response.json.as_bytes();
    u32::try_from(json.len()).map_err(|_| Status::LimitExceeded)?;
    let count = u32::try_from(response.payloads.len()).map_err(|_| Status::LimitExceeded)?;
    if count > 64 {
        return Err(Status::LimitExceeded);
    }
    let mut size = 8usize.checked_add(json.len()).ok_or(Status::LimitExceeded)?;
    for payload in &response.payloads {
        u32::try_from(payload.len()).map_err(|_| Status::LimitExceeded)?;
        size = size
            .checked_add(4)
            .and_then(|n| n.checked_add(payload.len()))
            .ok_or(Status::LimitExceeded)?;
    }
    if size > limit || size > isize::MAX as usize || size > u32::MAX as usize {
        return Err(Status::LimitExceeded);
    }
    Ok(size)
}

pub(crate) fn try_pack(response: &HostResponse, limit: usize) -> Result<Vec<u8>, Status> {
    let size = packed_len(response, limit)?;
    let json = response.json.as_bytes();
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(size).map_err(|_| Status::LimitExceeded)?;
    bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
    bytes.extend_from_slice(json);
    bytes.extend_from_slice(&(response.payloads.len() as u32).to_le_bytes());
    for payload in &response.payloads {
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
    }
    Ok(bytes)
}

pub(crate) struct OwnedResult {
    pub bytes: Vec<u8>,
    _engine: Option<Reservation>,
    _global: Reservation,
}

impl OwnedResult {
    pub(crate) fn from_response(
        response: &HostResponse,
        limit: u64,
        engine: Option<&Arc<ByteBudget>>,
    ) -> Result<Arc<Self>, Status> {
        static GLOBAL: OnceLock<Arc<ByteBudget>> = OnceLock::new();
        let size =
            packed_len(response, usize::try_from(limit).map_err(|_| Status::LimitExceeded)?)?;
        let global = GLOBAL.get_or_init(|| ByteBudget::new(1024 * 1024 * 1024));
        let global = global.reserve(size as u64)?;
        let engine = engine.map(|b| b.reserve(size as u64)).transpose()?;
        let bytes = try_pack(response, size)?;
        Ok(Arc::new(Self { bytes, _engine: engine, _global: global }))
    }
}

fn read_u32(bytes: &[u8], at: &mut usize) -> Result<u32, Status> {
    let end = at.checked_add(4).ok_or(Status::InvalidArgument)?;
    let value = bytes.get(*at..end).ok_or(Status::InvalidArgument)?;
    *at = end;
    Ok(u32::from_le_bytes(value.try_into().map_err(|_| Status::InvalidArgument)?))
}

pub(crate) fn unpack_payloads(bytes: &[u8], limit: usize) -> Result<Payloads, Status> {
    if bytes.len() > limit {
        return Err(Status::LimitExceeded);
    }
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let mut at = 0;
    let count = read_u32(bytes, &mut at)? as usize;
    if count > 64 {
        return Err(Status::InvalidArgument);
    }
    for _ in 0..count {
        let len = read_u32(bytes, &mut at)? as usize;
        at =
            at.checked_add(len).filter(|end| *end <= bytes.len()).ok_or(Status::InvalidArgument)?;
    }
    if at != bytes.len() {
        return Err(Status::InvalidArgument);
    }
    let mut payloads = Vec::new();
    payloads.try_reserve_exact(count).map_err(|_| Status::LimitExceeded)?;
    at = 4;
    for _ in 0..count {
        let len = read_u32(bytes, &mut at)? as usize;
        let mut payload = Vec::new();
        payload.try_reserve_exact(len).map_err(|_| Status::LimitExceeded)?;
        payload.extend_from_slice(&bytes[at..at + len]);
        at += len;
        payloads.push(payload);
    }
    Ok(payloads)
}

#[cfg(test)]
mod tests {
    use super::*;
    use epdf_engine::host::HostResponse;

    #[test]
    fn reservations_refuse_excess_and_restore_capacity_on_drop() {
        let budget = ByteBudget::new(10);
        let a = budget.reserve(7).unwrap();
        assert!(matches!(budget.reserve(4), Err(Status::LimitExceeded)));
        let b = budget.reserve(3).unwrap();
        assert!(matches!(budget.reserve(1), Err(Status::LimitExceeded)));
        drop(a);
        let c = budget.reserve(7).unwrap();
        drop((b, c));
        assert!(budget.reserve(10).is_ok());
        assert!(matches!(budget.reserve(u64::MAX), Err(Status::LimitExceeded)));
    }

    #[test]
    fn response_framing_matches_the_wire_and_rejects_over_budget_before_copying() {
        let response =
            HostResponse { json: "{\"result\":7}".into(), payloads: vec![vec![1, 2], vec![]] };
        assert!(matches!(try_pack(&response, 29), Err(Status::LimitExceeded)));
        let bytes = try_pack(&response, 30).unwrap();
        assert_eq!(&bytes[..4], &[12, 0, 0, 0]);
        assert_eq!(&bytes[4..16], b"{\"result\":7}");
        assert_eq!(&bytes[16..], &[2, 0, 0, 0, 2, 0, 0, 0, 1, 2, 0, 0, 0, 0]);
    }

    #[test]
    fn payload_frames_reject_truncation_excess_count_and_trailing_bytes() {
        assert_eq!(unpack_payloads(&[0, 0, 0, 0], 16).unwrap(), Vec::<Vec<u8>>::new());
        for bytes in [
            &[1, 0, 0][..],
            &[65, 0, 0, 0][..],
            &[1, 0, 0, 0, 255, 255, 255, 255][..],
            &[0, 0, 0, 0, 99][..],
        ] {
            assert!(unpack_payloads(bytes, 16).is_err());
        }
        assert!(matches!(unpack_payloads(&[0, 0, 0, 0], 3), Err(Status::LimitExceeded)));
    }
}

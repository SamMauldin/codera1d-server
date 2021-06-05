use chrono::{serde::ts_seconds, DateTime, Duration, Utc};
use lazy_static::lazy_static;
use roaring::RoaringBitmap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::{
    iter::FromIterator,
    ops::{BitOrAssign, BitXorAssign},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeReservation {
    pub codes: Vec<String>,
    #[serde(with = "ts_seconds")]
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Raid {
    #[serde(serialize_with = "bitmap_to_bytes")]
    #[serde(deserialize_with = "bitmap_from_bytes")]
    remaining_codes: RoaringBitmap,
    #[serde(serialize_with = "bitmap_to_bytes")]
    #[serde(deserialize_with = "bitmap_from_bytes")]
    tried_codes: RoaringBitmap,
    code_reservations: Vec<CodeReservation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaidInfo {
    pub remaining_code_count: u32,
    pub tried_code_count: u32,
}

lazy_static! {
    static ref PIN_CODE_LIST: Vec<String> = include_str!("pin_codes.csv")
        .lines()
        .flat_map(|line| line.split(";").next())
        .map(str::to_owned)
        .collect();
}

impl Raid {
    pub fn new() -> Raid {
        let mut bitmap = RoaringBitmap::new();
        bitmap.insert_range(0..10_000);

        Raid {
            remaining_codes: bitmap,
            tried_codes: Default::default(),
            code_reservations: Default::default(),
        }
    }

    pub fn expire_reservations(&mut self) {
        let now = Utc::now();

        let expired_reservations = self
            .code_reservations
            .drain_filter(|reservation| reservation.expires_at < now)
            .collect::<Vec<_>>();

        let codes_to_retry: Vec<u32> = expired_reservations
            .into_iter()
            .flat_map(|reservation| reservation.codes)
            .filter(|code| !self.tried_codes.contains(string_to_code_index(code)))
            .map(|code| string_to_code_index(&code))
            .collect();

        self.remaining_codes
            .bitor_assign(RoaringBitmap::from_iter(codes_to_retry));
    }

    pub fn reserve_codes(&mut self, count: usize) -> CodeReservation {
        let mut codes = Vec::new();

        for _ in 0..count {
            if let Some(code) = self.remaining_codes.min() {
                self.remaining_codes.remove(code);
                codes.push(code_index_to_string(code).to_owned());
            }
        }

        codes.reverse();

        let expires_at = Utc::now() + Duration::minutes(1);

        let reservation = CodeReservation { codes, expires_at };

        self.code_reservations.push(reservation.clone());

        reservation
    }

    pub fn try_code(&mut self, code: String) {
        let code_idx = string_to_code_index(&code);
        self.remaining_codes.remove(code_idx);
        self.tried_codes.insert(code_idx);
    }

    pub fn skip_codes(&mut self, skip_count: u64) {
        let skipped_codes =
            RoaringBitmap::from_iter(self.remaining_codes.iter().take(skip_count as usize));

        self.remaining_codes.bitxor_assign(&skipped_codes);
        self.tried_codes.bitor_assign(skipped_codes);
    }
}

impl Into<RaidInfo> for &Raid {
    fn into(self) -> RaidInfo {
        let tried_codes = self.tried_codes.len() as u32;
        RaidInfo {
            tried_code_count: tried_codes,
            remaining_code_count: 10000 - tried_codes,
        }
    }
}

fn bitmap_from_bytes<'de, D>(deserializer: D) -> Result<RoaringBitmap, D::Error>
where
    D: Deserializer<'de>,
{
    let str: String = Deserialize::deserialize(deserializer)?;
    let bytes = base64::decode(str).map_err(serde::de::Error::custom)?;
    RoaringBitmap::deserialize_from(&bytes[..]).map_err(serde::de::Error::custom)
}

fn bitmap_to_bytes<S>(x: &RoaringBitmap, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut bytes = vec![];
    x.serialize_into(&mut bytes)
        .map_err(serde::ser::Error::custom)?;
    s.serialize_str(&base64::encode(&bytes))
}

fn code_index_to_string(code_idx: u32) -> &'static String {
    PIN_CODE_LIST.get(code_idx as usize).unwrap()
}

fn string_to_code_index(code_str: &str) -> u32 {
    PIN_CODE_LIST
        .iter()
        .enumerate()
        .find(|(_, code)| *code == code_str)
        .unwrap()
        .0 as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_raid() {
        let raid = Raid::new();
        assert_eq!(raid.tried_codes.len(), 0);
        assert_eq!(raid.remaining_codes.len(), 10_000);
    }

    #[test]
    fn skip_codes() {
        let mut raid = Raid::new();
        raid.skip_codes(1000);
        assert_eq!(raid.tried_codes.len(), 1000);
        assert_eq!(raid.remaining_codes.len(), 9_000);
        assert_eq!(raid.remaining_codes.min(), Some(1000));
    }

    #[test]
    fn try_code() {
        let mut raid = Raid::new();
        raid.try_code(String::from("1234"));
        assert_eq!(raid.tried_codes.len(), 1);
        assert_eq!(raid.remaining_codes.len(), 9_999);
        assert_eq!(raid.tried_codes.min(), Some(0));
        assert_eq!(
            raid.remaining_codes
                .iter()
                .find(|&code_idx| code_index_to_string(code_idx) == &String::from("1234")),
            None
        );
    }

    #[test]
    fn reserve_codes() {
        let mut raid = Raid::new();
        let reservation = raid.reserve_codes(5);
        assert_eq!(raid.tried_codes.len(), 0);
        assert_eq!(raid.remaining_codes.len(), 9_995);
        assert_eq!(
            reservation.codes,
            vec!["1234", "1111", "0000", "1212", "7777"]
        );
    }

    #[test]
    fn expire_reservations_untried() {
        let mut raid = Raid::new();
        let mut reservation = raid.reserve_codes(5);
        reservation.expires_at = Utc::now() - Duration::minutes(1);
        raid.code_reservations = vec![reservation];
        raid.expire_reservations();

        assert_eq!(raid.tried_codes.len(), 0);
        assert_eq!(raid.remaining_codes.len(), 10_000);
    }

    #[test]
    fn expire_reservations_tried() {
        let mut raid = Raid::new();
        let mut reservation = raid.reserve_codes(5);
        raid.try_code(reservation.codes.pop().unwrap());
        reservation.expires_at = Utc::now() - Duration::minutes(1);
        raid.code_reservations = vec![reservation];
        raid.expire_reservations();

        assert_eq!(raid.tried_codes.len(), 1);
        assert_eq!(raid.remaining_codes.len(), 9_999);
    }

    #[test]
    fn test_code_index_to_string() {
        assert_eq!(code_index_to_string(0), "1234");
    }

    #[test]
    fn test_string_to_code_index() {
        assert_eq!(string_to_code_index("1234"), 0);
    }
}

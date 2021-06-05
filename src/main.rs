#![feature(proc_macro_hygiene, decl_macro, drain_filter)]

#[macro_use]
extern crate rocket;
#[macro_use]
extern crate anyhow;

mod raid;

use anyhow::Result;
use lazy_static::lazy_static;
use raid::{CodeReservation, Raid, RaidInfo};
use rocket::{
    outcome::IntoOutcome,
    request::{self, FromRequest, Request},
    State,
};
use rocket_contrib::json::Json;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env,
    sync::{Arc, Mutex},
};

lazy_static! {
    static ref API_KEY: String = env::var("CODERA1D_API_KEY").unwrap();
}

struct ApiKey<'r>(&'r str);

#[derive(Debug)]
enum ApiKeyError {
    Missing,
    Invalid,
}

impl<'a, 'r> FromRequest<'a, 'r> for ApiKey<'a> {
    type Error = ApiKeyError;

    fn from_request(request: &'a Request<'r>) -> request::Outcome<ApiKey<'a>, ApiKeyError> {
        request
            .headers()
            .get_one("X-Api-Key")
            .ok_or(ApiKeyError::Missing)
            .and_then(|key| {
                if key == *API_KEY {
                    Ok(ApiKey(key))
                } else {
                    Err(ApiKeyError::Invalid)
                }
            })
            .into_outcome(rocket::http::Status::Forbidden)
    }
}

type RaidState = Arc<Mutex<RaidMap>>;

#[derive(Debug, Serialize, Deserialize)]
struct RaidMap {
    raids: HashMap<String, Raid>,
}

type PubRaidMap = HashMap<String, RaidInfo>;

impl RaidMap {
    fn load() -> Result<RaidMap> {
        let raids_bin = std::fs::read("data/raids.bin")?;
        let raid_map = bincode::deserialize_from(&raids_bin[..])?;

        Ok(raid_map)
    }

    fn save(&self) -> Result<()> {
        let serialized = bincode::serialize(self)?;

        std::fs::write("data/raids.bin", serialized)?;

        Ok(())
    }

    fn to_pub_json(&self) -> Json<PubRaidMap> {
        let pub_raid_map = self
            .raids
            .iter()
            .map(|(name, raid)| (name.clone(), raid.into()))
            .collect();

        Json(pub_raid_map)
    }
}

impl Default for RaidMap {
    fn default() -> Self {
        RaidMap {
            raids: HashMap::new(),
        }
    }
}

#[get("/")]
fn index(_key: ApiKey) -> String {
    "Welcome to codera1d".to_owned()
}

#[get("/raids")]
fn raid_list(state: State<RaidState>, _key: ApiKey) -> Json<PubRaidMap> {
    let mut raid_state = state.lock().unwrap();
    let raids = &mut raid_state.raids;
    raids
        .iter_mut()
        .for_each(|(_, raid)| raid.expire_reservations());

    raid_state.to_pub_json()
}

#[derive(Deserialize)]
struct RaidReference {
    name: String,
    skip_count: Option<u64>,
}

#[post("/raids", data = "<form>")]
fn create_raid(
    form: Json<RaidReference>,
    state: State<RaidState>,
    _key: ApiKey,
) -> Result<Json<PubRaidMap>> {
    let mut raid_state = state.lock().unwrap();
    let raids = &mut raid_state.raids;

    if raids.contains_key(&form.name) {
        return Err(anyhow!("Raid already exists"));
    }

    let mut new_raid = Raid::new();

    if let Some(skip_count) = form.skip_count {
        new_raid.skip_codes(skip_count);
    }

    raids.insert(form.name.clone(), new_raid.clone());

    raid_state.save()?;

    Ok(raid_state.to_pub_json())
}

#[delete("/raids", data = "<form>")]
fn delete_raid(form: Json<RaidReference>, state: State<RaidState>, _key: ApiKey) -> Result<()> {
    let mut raid_state = state.lock().unwrap();
    let raids = &mut raid_state.raids;

    raids.remove(&form.name);

    raid_state.save()?;

    Ok(())
}

#[get("/raids/<name>")]
fn get_raid(name: String, state: State<RaidState>, _key: ApiKey) -> Result<Json<Raid>> {
    let mut raid_state = state.lock().unwrap();
    let raids = &mut raid_state.raids;

    let raid = raids.get(&name).ok_or(anyhow!("Raid not found"))?.clone();

    Ok(Json(raid))
}

#[post("/raids/<name>/reserve_codes")]
fn reserve_codes(
    name: String,
    state: State<RaidState>,
    _key: ApiKey,
) -> Result<Json<CodeReservation>> {
    let mut raid_state = state.lock().unwrap();
    let raids = &mut raid_state.raids;

    let raid = raids.get_mut(&name).ok_or(anyhow!("Raid not found"))?;

    let code_reservation = raid.reserve_codes(5);

    raid_state.save()?;

    Ok(Json(code_reservation))
}

#[derive(Deserialize)]
struct CodeInput {
    code: String,
}

#[post("/raids/<name>/try_code", data = "<form>")]
fn try_code(
    name: String,
    form: Json<CodeInput>,
    state: State<RaidState>,
    _key: ApiKey,
) -> Result<()> {
    let mut raid_state = state.lock().unwrap();
    let raids = &mut raid_state.raids;

    let raid = raids.get_mut(&name).ok_or(anyhow!("Raid not found"))?;

    raid.try_code(form.code.clone());

    raid_state.save()?;

    Ok(())
}

fn main() {
    let raid_map = RaidMap::load().unwrap_or_default();
    let raid_state: RaidState = Arc::new(Mutex::new(raid_map));

    rocket::ignite()
        .manage(raid_state)
        .mount(
            "/",
            routes![
                index,
                raid_list,
                get_raid,
                create_raid,
                delete_raid,
                reserve_codes,
                try_code
            ],
        )
        .launch();
}

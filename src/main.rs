extern crate hyper;
extern crate rustc_serialize;

use rustc_serialize::json;
use std::collections::{BTreeMap};
use std::env;
use std::io::Read;
use std::time::Duration;

use hyper::client::{Client};

mod threadthrottler;

// All we care about for every realm is its "slug".
#[derive(Debug, RustcDecodable)]
struct RealmInfo {
    slug: String,
}

// Contains all the price info for a realm.
struct RealmAuctionInfo {
    last_update: u64,  // The last time we got this info, as reported by the Blizzard API.
    price_points: BTreeMap<u64, Vec<(u32, u64)>>,  // Map of item ids to a vector of pairs of item quantity, buyout price.
}

// Represents the JSON reply from the auction data status endpoint.
#[derive(Debug, RustcDecodable)]
struct AuctionDataPointer {
    url: String,
    lastModified: u64,
}

#[derive(Debug, RustcDecodable)]
struct AuctionDataReply {
    files: Vec<AuctionDataPointer>,
}

fn main() {
    let tt = threadthrottler::ThreadThrottler::new(100, Duration::new(1, 0));
    let token = match env::args().nth(1) {
        Some(token) => token,
        None => {
            println!("Usage: bloodmoney <api token>");
            return;
        }
    };

    let client = Client::new();
    let mut res = client.get(
        &format!("https://us.api.battle.net/wow/realm/status?locale=en_US&apikey={}", token))
        .send().expect("Failed to download realm status.");
    assert_eq!(res.status, hyper::Ok);
    let mut s = String::new();
    res.read_to_string(&mut s).unwrap();
    let realm_data: BTreeMap<String, Vec<RealmInfo>> =
        json::decode(&s).expect("Malformed realm response.");
    let realm_names: Vec<_> = realm_data.get("realms")
        .expect("Malformed realm response.")
        .into_iter()
        .map(|r| r.slug.to_string())
        .collect();

    for realm_name in realm_names {
        res = client.get(&format!("https://us.api.battle.net/wow/auction/data/{}?locale=en_US&apikey={}", realm_name, token))
            .send()
            .expect(format!("Failed to get auction data status for {}.", realm_name).as_str());
        assert_eq!(res.status, hyper::Ok);
        s.clear();
        res.read_to_string(&mut s).unwrap();
        let auction_data_reply: AuctionDataReply = json::decode(&s).expect("Malformed json reply.");
    }
}

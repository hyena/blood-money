extern crate hyper;
extern crate rustc_serialize;
extern crate scoped_threadpool;

use rustc_serialize::json;
use std::collections::{BTreeMap, HashSet};
use std::env;
use std::io::Read;
use std::sync::Mutex;
use std::time::Duration;

use hyper::client::{Client, Response};
use scoped_threadpool::Pool;

mod threadthrottler;

// All we care about for every realm is its "slug".
#[derive(Debug, RustcDecodable)]
struct RealmInfo {
    slug: String,
    connected_realms: Vec<String>,
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
    let mut realm_data: BTreeMap<String, Vec<RealmInfo>> =
        json::decode(&s).expect("Malformed realm response.");
    let realm_infos: &mut Vec<RealmInfo> = realm_data.get_mut("realms").expect("Malformed realm response.");
    let realm_sets: BTreeMap<String, HashSet<String>> = realm_infos.into_iter().map(|r| (r.connected_realms[0].clone(), r.connected_realms.drain(..).collect::<HashSet<String>>()) ).collect();

    // Get all their auction data in parallel.
    let pointer_lock = Mutex::new(BTreeMap::new());

    let mut pool = Pool::new(10);
    pool.scoped(|scope| {
        for (lead_realm, realm_list) in realm_sets {
            scope.execute(|| {
                let mut succeeded = false;
                let mut retry = 0;
                let mut s = String::new();
                let mut res: Response;

                while !succeeded {
                    retry += 1;
                    tt.pass_through_or_block();
                    match client.get(&format!("https://us.api.battle.net/wow/auction/data/{}?locale=en_US&apikey={}", &lead_realm, &token))
                        .send() {
                            Ok(r) => res = r,
                            Err(e) => {
                                println!("Failed to get auction status for {}: {}. Retry {}.", &lead_realm, e, retry);
                                continue;
                            }
                        }
                    if res.status != hyper::Ok {
                        println!("Error downloading auction status for {}. Retry {}.", &lead_realm, retry);
                        continue;
                    }
                    match res.read_to_string(&mut s) {
                        Ok(_) => (),
                        Err(e) => {
                            println!("Failed to process auction status for {}: {}. Retry {}.", &lead_realm, e, retry);
                            continue;
                        }
                    }
                    succeeded = true;
                }
                let mut auction_data_reply: AuctionDataReply = json::decode(&s).expect("Malformed json reply.");
                let auction_data_pointer = auction_data_reply.files.pop().unwrap();

                // Download the auction data but don't do anything with it for now.
                println!("Opening {} for {}", &auction_data_pointer.url, &lead_realm);
                succeeded = false;
                retry = 0;
                s.clear();
                while !succeeded {
                    retry += 1;
                    tt.pass_through_or_block();  // Shouldn't be necessary because this isn't API linked, but be careful.
                    match client.get(&auction_data_pointer.url).send() {
                        Ok(r) => res = r,
                        Err(e) => {
                            println!("Error downloading data for {}: {}. Retry {}.", &lead_realm, e, retry);
                            continue;
                        }
                    }
                    if res.status != hyper::Ok {
                        println!("Error downloading data for {}. Retry {}.", &lead_realm, retry);
                        continue;
                    }
                    match res.read_to_string(&mut s) {
                        Ok(_) => (),
                        Err(e) => {
                            println!("Failed to process auction data for {}: {}. Retry {}.", &lead_realm, e, retry);
                            continue;
                        }
                    }
                    succeeded = true;
                }
                println!("Finished processing {}", &lead_realm);
                {
                    let mut pointers = pointer_lock.lock().unwrap();
                    pointers.insert(lead_realm, s);
                }
            })
        }
        scope.join_all();
    });
    let auction_pointers = pointer_lock.into_inner().unwrap();
    println!("Done!");
}

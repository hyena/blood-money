#![feature(proc_macro)]

extern crate hyper;
extern crate iron;
extern crate router;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate scoped_threadpool;
extern crate tera;

use std::collections::{BTreeMap, HashMap};
use std::env;
use std::rc::Rc;
use std::sync::{Arc, RwLock};

use iron::headers::ContentType;
use iron::prelude::*;
use iron::status;
use router::Router;
use hyper::client::{Client, Response as HyperResponse};
use scoped_threadpool::Pool;
use tera::{Context, Tera};

pub mod battle_net_api_client;
pub mod thread_throttler;

use battle_net_api_client::BattleNetApiClient;

// Contains all the price info for a realm.
struct RealmAuctionInfo {
    last_update: u64,  // The last time we got this info, as reported by the Blizzard API.
    price_points: BTreeMap<u64, Vec<(u32, u64)>>,  // Map of item ids to a vector of pairs of item quantity, buyout price.
}

// Represents the JSON reply from the auction data status endpoint.
#[derive(Debug)]
struct AuctionDataPointer {
    url: String,
    lastModified: u64,
}

#[derive(Debug)]
struct AuctionDataReply {
    files: Vec<AuctionDataPointer>,
}

/// Represents a single option available for sale from the blood vendor.
#[derive(Debug, Deserialize)]
struct BloodVendorItem {
    name: String,
    quantity: u64,
    id: u64,
}

/// The calculated values for items on a particular realm.
#[derive(Debug)]
struct CurrentRealmValues {
    last_update: u64,  // The last time we got this info, as reported by the Blizzard API.
    auction_values: Vec<(u64, u64)>,  // Sorted vec of (item id, value)
}

/// All the data in a single row in our price list for a realm.
#[derive(Debug, Serialize)]
struct PriceRow {
    name: String,
    quantity: u64,
    icon: String,
    value: u64,
    gold: u64,
    silver: u64,
    copper: u64,
}

fn main() {
    let token = match env::args().nth(1) {
        Some(token) => token,
        None => {
            println!("Usage: bloodmoney <api token>");
            return;
        }
    };
    let client = BattleNetApiClient::new(&token);

    // Process our item options and grab their icon names.
    let items: Vec<BloodVendorItem> = serde_json::from_str(include_str!("../catalog/items.json"))
        .expect("Error reading items.");
    let item_id_map: Arc<HashMap<u64, BloodVendorItem>> = Arc::new(items.into_iter().map(|x| (x.id, x)).collect());
    let item_icons: Arc<HashMap<u64, String>> = Arc::new(item_id_map.keys().map(|&id| (id, client.get_item_info(id).icon)).collect());

    // Get the list of realms and create an empty price map so we can render pages while
    // waiting for the auction results to be retrieved.
    let realms = Arc::new(client.get_realms());
    let price_map: Arc<BTreeMap<String, RwLock<CurrentRealmValues>>> =
        Arc::new(BattleNetApiClient::process_realm_sets(&realms).iter().flat_map(|realm_set|
            realm_set.iter().map(|realm_name| (realm_name.clone(), RwLock::new(CurrentRealmValues {
                last_update: 0,
                auction_values: item_id_map.keys().map(|id| (*id, 0u64)).collect(),
            }))
        )).collect());

    // Set up our web-app.
    let tera = Arc::new(Tera::new("templates/**/*"));
    let mut router = Router::new();
    {
        let realms = realms.clone();
        let tera = tera.clone();
        router.get("/", move |_: &mut Request| {
            let mut context = Context::new();
            context.add("realms", &realms);
            Ok(Response::with((ContentType::html().0, status::Ok, tera.render("index.html", context).unwrap())))
        }, "index");
    }
    {
        let price_map = price_map.clone();
        let item_id_map = item_id_map.clone();
        let realms = realms.clone();
        let tera = tera.clone();
        router.get("/:realm", move |req : &mut Request| {
            let realm = req.extensions.get::<Router>().unwrap().find("realm").unwrap();
            if let Some(realm_prices_lock) = price_map.get(realm) {
                let mut context = Context::new();
                let realm_prices = realm_prices_lock.read().unwrap();
                // Build up a list of entries.
                let price_rows: Vec<PriceRow> = realm_prices.auction_values.iter().map(|&(id, value)| {
                    let item_info = item_id_map.get(&id).unwrap();
                    let gold = value / (10_000);
                    let silver = (value - gold * 10_000) / 100;
                    let copper = value - gold * 10_000 - silver * 100;
                    PriceRow {
                        name: item_info.name.clone(),
                        quantity: item_info.quantity,
                        icon: item_icons.get(&id).unwrap().clone(),
                        value: value,
                        gold: gold,
                        silver: silver,
                        copper: copper,
                    }
                }).collect();
                context.add("realm_name", &realms.iter().find(|&realm_info| &realm_info.slug == realm).unwrap().name);
                context.add("price_rows", &price_rows);
                Ok(Response::with((ContentType::html().0, status::Ok, tera.render("prices.html", context).unwrap())))
            } else {
                return Ok(Response::with(status::NotFound));
            }
        }, "realm-prices");
    }
    //router.get("/:realm", realm_handler, "realm");
    println!("Ready for web traffic.");
    Iron::new(router).http("localhost:3000").unwrap();
    // // Get all their auction data in parallel.
    // let pointer_lock = Mutex::new(BTreeMap::new());
    //
    // let mut pool = Pool::new(10);
    // pool.scoped(|scope| {
    //     for (lead_realm, realm_list) in realm_sets {
    //         scope.execute(|| {
    //             let mut succeeded = false;
    //             let mut retry = 0;
    //             let mut s = String::new();
    //             let mut res: Response;
    //
    //             while !succeeded {
    //                 retry += 1;
    //                 tt.pass_through_or_block();
    //                 match client.get(&format!("https://us.api.battle.net/wow/auction/data/{}?locale=en_US&apikey={}", &lead_realm, &token))
    //                     .send() {
    //                         Ok(r) => res = r,
    //                         Err(e) => {
    //                             println!("Failed to get auction status for {}: {}. Retry {}.", &lead_realm, e, retry);
    //                             continue;
    //                         }
    //                     }
    //                 if res.status != hyper::Ok {
    //                     println!("Error downloading auction status for {}. Retry {}.", &lead_realm, retry);
    //                     continue;
    //                 }
    //                 match res.read_to_string(&mut s) {
    //                     Ok(_) => (),
    //                     Err(e) => {
    //                         println!("Failed to process auction status for {}: {}. Retry {}.", &lead_realm, e, retry);
    //                         continue;
    //                     }
    //                 }
    //                 succeeded = true;
    //             }
    //             let mut auction_data_reply: AuctionDataReply = json::decode(&s).expect("Malformed json reply.");
    //             let auction_data_pointer = auction_data_reply.files.pop().unwrap();
    //
    //             // Download the auction data but don't do anything with it for now.
    //             println!("Opening {} for {}", &auction_data_pointer.url, &lead_realm);
    //             succeeded = false;
    //             retry = 0;
    //             s.clear();
    //             while !succeeded {
    //                 retry += 1;
    //                 tt.pass_through_or_block();  // Shouldn't be necessary because this isn't API linked, but be careful.
    //                 match client.get(&auction_data_pointer.url).send() {
    //                     Ok(r) => res = r,
    //                     Err(e) => {
    //                         println!("Error downloading data for {}: {}. Retry {}.", &lead_realm, e, retry);
    //                         continue;
    //                     }
    //                 }
    //                 if res.status != hyper::Ok {
    //                     println!("Error downloading data for {}. Retry {}.", &lead_realm, retry);
    //                     continue;
    //                 }
    //                 match res.read_to_string(&mut s) {
    //                     Ok(_) => (),
    //                     Err(e) => {
    //                         println!("Failed to process auction data for {}: {}. Retry {}.", &lead_realm, e, retry);
    //                         continue;
    //                     }
    //                 }
    //                 succeeded = true;
    //             }
    //             println!("Finished processing {}", &lead_realm);
    //             {
    //                 let mut pointers = pointer_lock.lock().unwrap();
    //                 pointers.insert(lead_realm, s);
    //             }
    //         })
    //     }
    //     scope.join_all();
    // });
    // let auction_pointers = pointer_lock.into_inner().unwrap();
    // println!("Done!");
}

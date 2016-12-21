#![feature(proc_macro, slice_patterns)]

extern crate hyper;
extern crate iron;
extern crate regex;
extern crate router;
extern crate regex;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_json;
extern crate scoped_threadpool;
extern crate tera;

use std::collections::{BTreeMap, HashMap};
use std::env;
use std::sync::{Arc, RwLock};
use std::thread::sleep;
use std::time::{Instant, Duration, SystemTime, UNIX_EPOCH};

use iron::headers::ContentType;
use iron::prelude::*;
use iron::status;
use router::Router;
use scoped_threadpool::Pool;
use tera::{Context, Tera};

pub mod battle_net_api_client;
pub mod thread_throttler;

use battle_net_api_client::{AuctionListing, BattleNetApiClient, Region};

/// Represents a single option available for sale from the blood vendor.
#[derive(Debug, Deserialize)]
struct BloodVendorItem {
    name: String,
    quantity: u64,
    id: u64,
}

/// Value of an item on a realm.
#[derive(Debug)]
struct ItemValue {
    id: u64,
    value: u64,
}

/// The calculated values for items on a particular realm.
#[derive(Debug)]
struct CurrentRealmValues {
    last_update: u64,  // The last time we got this info, as reported by the Blizzard API.
    auction_values: Arc<Vec<ItemValue>>,  // Should be sorted by value.
}

/// All the data in a single row in our price list for a realm.
#[derive(Debug, Serialize)]
struct PriceRow {
    name: String,
    quantity: u64,
    icon: String,
    value_ratio: u64,
    gold: u64,
    silver: u64,
    copper: u64,
}

/// Number of threads to use when fetching auction house results.
const NUM_AUCTION_DATA_THREADS: u32 = 5;

/// Number of seconds to wait between fetching new auction results.
const RESULT_FETCH_PERIOD: u64 = 60 * 30;

/// Given a vec of auction listings for a realm and a map of the items we care about,
/// returns a vec of (item_id, value) sorted by decreasing value, where value is
/// based on the 5th percentile buyout price.
fn calculate_auction_values(listings: &Vec<AuctionListing>, items: &HashMap<u64, BloodVendorItem>) -> Vec<ItemValue> {
    // Calculate 5th percentiles for the items we care about.
    let mut price_points: BTreeMap<u64, Vec<(u64, u64)>> = BTreeMap::new();
    for listing in listings {
        if items.contains_key(&listing.item) && listing.buyout > 0 {
            price_points.entry(listing.item).or_insert(Vec::new()).push((listing.quantity, listing.buyout / listing.quantity));
        }
    }
    for quantities_and_buyouts in price_points.values_mut() {
        quantities_and_buyouts.sort_by_key(|a| a.1);  // Sort by buyout price.
    }
    let total_item_quantities: BTreeMap<u64, u64> =
        price_points.iter().map(|(k, v)| {
            (*k, v.iter().fold(0, |sum, quantity_and_buyout| sum + quantity_and_buyout.0))
        }).collect();
    let fifth_percentile_price_points: BTreeMap<u64, u64> =
        price_points.iter().map(|(item_id, ref item_listings)| {
            let fifth_percentile_quantity = total_item_quantities.get(item_id).unwrap() / 20;
            let mut running_sum: u64 = 0;
            let fifth_percentile_listing = item_listings.iter().find(|&&(quantity, _)| {
                running_sum += quantity;
                running_sum >= fifth_percentile_quantity
            }).unwrap();
            (*item_id, fifth_percentile_listing.1)
        }).collect();
    let mut item_values: Vec<ItemValue> = items.values().map(|item|
        ItemValue {
            id: item.id,
            value: fifth_percentile_price_points.get(&item.id).unwrap_or(&0u64) * item.quantity,
        }
    ).collect();
    item_values.sort_by_key(|item_value| !item_value.value);
    item_values
}

/// Given a battle net Region, return the first path of the app's URL.
fn app_url_for_region(region: &Region) -> &'static str {
    match region {
        &Region::US => "blood-money",
        &Region::EU => "blood-money-eu",
    }
}

fn main() {
    let token = match env::args().nth(1) {
        Some(token) => token,
        None => {
            println!("Usage: bloodmoney <api token> (us|eu)");
            return;
        }
    };
    let locale = match env::args().nth(2) {
        Some(ref s) if s == "us" => Region::US,
        Some(ref s) if s == "eu" => Region::EU,
        _ => {
            println!("Usage: bloodmoney <api token> (us|eu)");
            return;
        }
    };
    let client = Arc::new(BattleNetApiClient::new(&token, locale));

    // Process our item options and grab their icon names.
    let items: Vec<BloodVendorItem> = serde_json::from_str(include_str!("../catalog/items.json"))
        .expect("Error reading items.");
    let item_id_map: Arc<HashMap<u64, BloodVendorItem>> = Arc::new(items.into_iter().map(|x| (x.id, x)).collect());
    let item_icons: Arc<HashMap<u64, String>> = Arc::new(item_id_map.keys().map(|&id| (id, client.get_item_info(id).icon)).collect());

    // Get the list of realms and create an empty price map so we can render pages while
    // waiting for the auction results to be retrieved.
    let realms = Arc::new(client.get_realms());
    let connected_realms = BattleNetApiClient::process_connected_realms(&realms);
    let price_map: Arc<BTreeMap<String, RwLock<CurrentRealmValues>>> =
        Arc::new(realms.iter().map(|realm| (realm.slug.clone(), RwLock::new(CurrentRealmValues {
            last_update: 0,
            auction_values: Arc::new(item_id_map.keys().map(|id| ItemValue{id: *id, value: 0u64}).collect()),
        }))).collect());

    // Set up our web-app.
    let tera = Arc::new(Tera::new("templates/**/*"));
    let mut router = Router::new();
    {
        let realms = realms.clone();
        let tera = tera.clone();
        router.get(format!("/{}", app_url_for_region(&locale)), move |_: &mut Request| {
            let mut context = Context::new();
            context.add("realms", &realms);
            context.add("is_eu", &(locale == Region::EU));
            Ok(Response::with((ContentType::html().0, status::Ok, tera.render("index.html", context).unwrap())))
        }, "index");
    }
    {
        let price_map = price_map.clone();
        let item_id_map = item_id_map.clone();
        let realms = realms.clone();
        let tera = tera.clone();
        router.get(format!("/{}/:realm", app_url_for_region(&locale)), move |req : &mut Request| {
            let realm = req.extensions.get::<Router>().unwrap().find("realm").unwrap();
            if let Some(realm_prices_lock) = price_map.get(realm) {
                let mut context = Context::new();
                let realm_prices = realm_prices_lock.read().unwrap();
                // Build up a list of entries.
                let highest_value = realm_prices.auction_values.get(0).unwrap().value;
                let price_rows: Vec<PriceRow> = realm_prices.auction_values.iter().map(|&ItemValue{id, value}| {
                    let item_info = item_id_map.get(&id).unwrap();
                    let gold = value / (10_000);
                    let silver = (value - gold * 10_000) / 100;
                    let copper = value - gold * 10_000 - silver * 100;
                    let value_ratio = match highest_value {
                        0u64 => 0u64,
                        _ => value*100/highest_value,  // Percentile!
                    };
                    PriceRow {
                        name: item_info.name.clone(),
                        quantity: item_info.quantity,
                        icon: item_icons.get(&id).unwrap().clone(),
                        value_ratio: value_ratio,
                        gold: gold,
                        silver: silver,
                        copper: copper,
                    }
                }).collect();
                context.add("realm_name", &realms.iter().find(|&realm_info| &realm_info.slug == realm).unwrap().name);
                context.add("price_rows", &price_rows);
                // TODO: Change this to something more human readable.
                if realm_prices.last_update == 0 {
                    context.add("update_age", &-1);
                } else {
                    context.add("update_age",
                        &((SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() - realm_prices.last_update / 1000) / 60));
                }
                context.add("is_eu", &(locale == Region::EU));
                Ok(Response::with((ContentType::html().0, status::Ok, tera.render("prices.html", context).unwrap())))
            } else {
                return Ok(Response::with(status::NotFound));
            }
        }, "realm-prices");
    }
    let http_result = Iron::new(router).http(format!("localhost:{}", match locale {
        Region::US => 3000,
        Region::EU => 3001,
    }).as_str());
    println!("Ready for web traffic.");

    // Now that the webserver is up, periodically fetch
    // new auction house data.
    let mut pool = Pool::new(NUM_AUCTION_DATA_THREADS);
    loop {
        let download_start = Instant::now();
        let next_download_time = download_start + Duration::from_secs(RESULT_FETCH_PERIOD);
        println!("Starting download of auction data.");
        pool.scoped(|scope| {
            for realm_list in &connected_realms {
                // We have to move realm_list into the closure.
                // Clone other values.
                let client = client.clone();
                let price_map = price_map.clone();
                let item_id_map = item_id_map.clone();
                scope.execute(move || {
                    let lead_realm = realm_list.get(0).unwrap();
                    let update_time: u64;
                    let auction_listings: Vec<AuctionListing>;
                    println!("Downloading {}", lead_realm);
                    {
                        let current_realm_values =
                            price_map.get(lead_realm).unwrap().read().unwrap();
                        match client.get_auction_listings(lead_realm, current_realm_values.last_update) {
                            Some((ts, al)) => {
                                update_time = ts;
                                auction_listings = al;
                            },
                            None => return,
                        }
                    }
                    let auction_values = Arc::new(calculate_auction_values(&auction_listings, &item_id_map));
                    for realm in realm_list {
                        println!("Updating {}", realm);
                        let mut current_realm_values =
                            price_map.get(realm).unwrap().write().unwrap();
                        current_realm_values.auction_values = auction_values.clone();
                        current_realm_values.last_update = update_time;
                    }
                })
            }
            scope.join_all();
        });
        let download_end_time = Instant::now();
        println!("Downloading all realms took {} seconds.", download_end_time.duration_since(download_start).as_secs());
        if download_end_time < next_download_time {
            println!("Sleeping for {}", next_download_time.duration_since(download_end_time).as_secs());
            sleep(next_download_time.duration_since(download_end_time));
        }
    }
}

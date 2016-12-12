extern crate hyper;
extern crate rustc_serialize;
extern crate scoped_threadpool;

use rustc_serialize::json;
use std::collections::{BTreeMap, HashMap};
use std::env;
use std::io::Read;

use hyper::client::{Client, Response};

/// All we care about for every realm is its "slug".
#[derive(Debug, RustcDecodable)]
struct RealmInfo {
    name: String,
    slug: String,
    connected_realms: Option<Vec<String>>,
}

/// Contains our calculated price info for a realm.
#[derive(Debug)]
struct RealmAuctionInfo {
    last_update: u64,  // The last time we got this info, as reported by the Blizzard API.
    price_points: BTreeMap<u64, Vec<(u64, u64)>>,  // Map of item ids to a vector of pairs of item quantity, buyout price.
}

/// Represents the JSON reply from the auction data status endpoint.
#[derive(Debug, RustcDecodable)]
#[allow(non_snake_case)]
struct AuctionDataPointer {
    url: String,
    lastModified: u64,
}

#[derive(Debug, RustcDecodable)]
struct AuctionDataReply {
    files: Vec<AuctionDataPointer>,
}

/// The fields we care about in blizzard's auction reply.
#[derive(Debug, RustcDecodable)]
struct AuctionListing {
    item: u64,
    buyout: u64,
    quantity: u64,
}

/// Represents the reply from blizzard's auction data urls.
#[derive(Debug, RustcDecodable)]
struct AuctionListingsData {
    realms: Vec<RealmInfo>,
    auctions: Vec<AuctionListing>,
}

/// Represents a single item for sale from the blood vendor.
#[derive(Debug, RustcDecodable)]
struct BloodVendorItem {
    name: String,
    quantity: u64,
    id: u64,
}

// /// Given a vector of (quanity, buyout) tuples, returns the lowest buyout and the 10th percentile buyout.
// /// TODO: Make percentile parameterized
// fn calculate_lowest_buyout_and_10th_percentile(quantities_and_prices: &Vec<(u64, u64)>) -> (u64, u64) {
//
// }

fn main() {
    let target = "earthen-ring";
    let token = match env::args().nth(1) {
        Some(token) => token,
        None => {
            println!("Usage: blood-money <api token>");
            return;
        }
    };

    let items: Vec<BloodVendorItem> = json::decode(include_str!("../../catalog/items.json")).expect("Error reading items.");
    let item_ids: HashMap<u64, BloodVendorItem> = items.into_iter().map(|x| (x.id, x)).collect();

    let client = Client::new();
    let mut succeeded = false;
    let mut retry = 0;
    let mut s = String::new();

    while !succeeded {
        retry += 1;
        let mut res: Response;
        match client.get(&format!("https://us.api.battle.net/wow/auction/data/{}?locale=en_US&apikey={}", &target, &token))
            .send() {
                Ok(r) => res = r,
                Err(e) => {
                    println!("Failed to get auction status for {}: {}. Retry {}.", &target, e, retry);
                    continue;
                }
            }
        if res.status != hyper::Ok {
            println!("Error downloading auction status for {}. Retry {}.", &target, retry);
            continue;
        }
        match res.read_to_string(&mut s) {
            Ok(_) => (),
            Err(e) => {
                println!("Failed to process auction status for {}: {}. Retry {}.", &target, e, retry);
                continue;
            }
        }
        succeeded = true;
    }
    let mut auction_data_reply: AuctionDataReply = json::decode(&s).expect("Malformed json reply.");
    let auction_data_pointer = auction_data_reply.files.pop().unwrap();

    // Download the auction data but don't do anything with it for now.
    println!("Opening {} for {}", &auction_data_pointer.url, &target);
    succeeded = false;
    retry = 0;
    s.clear();
    while !succeeded {
        let mut res: Response;
        retry += 1;
        match client.get(&auction_data_pointer.url).send() {
            Ok(r) => res = r,
            Err(e) => {
                println!("Error downloading data for {}: {}. Retry {}.", &target, e, retry);
                continue;
            }
        }
        if res.status != hyper::Ok {
            println!("Error downloading data for {}. Retry {}.", &target, retry);
            continue;
        }
        match res.read_to_string(&mut s) {
            Ok(_) => (),
            Err(e) => {
                println!("Failed to process auction data for {}: {}. Retry {}.", &target, e, retry);
                continue;
            }
        }
        succeeded = true;
    }
    let auction_listings_data: AuctionListingsData = json::decode(&s).unwrap();

    let mut realm_auction_info = RealmAuctionInfo {
        last_update: auction_data_pointer.lastModified,
        price_points: BTreeMap::new(),
    };

    for listing in &auction_listings_data.auctions {
        if item_ids.contains_key(&listing.item) && listing.buyout > 0 {
            realm_auction_info.price_points.entry(listing.item).or_insert(Vec::new()).push((listing.quantity, listing.buyout));
        }
    }

    // Calculate 5th percentiles.
    for listings in realm_auction_info.price_points.values_mut() {
        listings.sort_by_key(|a| a.1);
    }
    let total_item_quantities: BTreeMap<u64, u64> = realm_auction_info.price_points.iter()
        .map(|(k, v)| (*k, v.iter().fold(0, |sum, auction| sum + auction.0))).collect();
    let fifth_percentile_price_points: BTreeMap<u64, u64> = realm_auction_info.price_points.iter()
        .map(|(item_id, ref item_listings)| {
            let fifth_percentile_quantity = total_item_quantities.get(item_id).unwrap() / 20;
            let mut running_sum: u64 = 0;
            let fifth_percentile_listing = item_listings.iter().find(|&&(quantity, buyout)| {
                running_sum += quantity;
                if running_sum >= fifth_percentile_quantity {
                    println!("{}: ({}, {})", item_ids.get(item_id).unwrap().name, quantity, buyout);
                }
                running_sum >= fifth_percentile_quantity
            }).unwrap();
            (*item_id, fifth_percentile_listing.1 / fifth_percentile_listing.0)
        }).collect();
    for (item_id, item) in &item_ids {
        println!("There are {} {} on the auction house and their 5th percentile buyout is {}.",
            total_item_quantities.get(&item_id).unwrap_or(&0u64),
            &item.name,
            fifth_percentile_price_points.get(&item_id).unwrap_or(&0u64));
    }

    let mut values: Vec<(String, u64)> = item_ids.values().map(|item|
        (format!("{}x{}", item.name, item.quantity),
         fifth_percentile_price_points.get(&item.id).unwrap_or(&0u64) * item.quantity)).collect();
    values.sort_by_key(|a| !a.1);
    println!("Best values: {:?}", values);

//    println!("Finished processing {}: {:?}", &target, realm_auction_info);
}

//! A Module for accessing Blizzard's WoW API.
//! The exposed functionality of this module has been structured
//! around the particular needs of blood-money: Not all fields are
//! represented and it's probably not generally useful.
//! TODO: Add support for locales.
extern crate hyper;
extern crate serde_json;

use std::collections::BTreeMap;
use std::io::Read;
use std::time::Duration;

use hyper::client::{Client, Response};
use regex::bytes::Regex;
use serde::de::Deserialize;
use thread_throttler::ThreadThrottler;

/// The content we care about in the realm status response.
#[derive(Debug, Serialize, Deserialize)]
pub struct RealmInfo {
    pub name: String,
    pub slug: String,
    pub connected_realms: Vec<String>,
}

/// Content we care about in an item info response.
#[derive(Debug, Deserialize)]
pub struct ItemInfo {
    pub id: u64,
    pub name: String,
    pub icon: String,
}

/// Represents the reply from blizzard's auction data urls.
#[derive(Debug, Deserialize)]
struct AuctionListingsReply {
    realms: Vec<BTreeMap<String, String>>,  // Can't re-use RealmInfo because no connected_realms.
    auctions: Vec<AuctionListing>,
}

/// Represents the JSON reply from the auction data status endpoint.
#[derive(Debug, Deserialize)]
#[allow(non_snake_case)]
struct AuctionDataPointer {
    url: String,
    lastModified: u64,
}

#[derive(Debug, Deserialize)]
struct AuctionDataReply {
    files: Vec<AuctionDataPointer>, // Will always be 1 element.
}

/// The fields we care about in blizzard's auction reply.
#[derive(Debug, Deserialize)]
pub struct AuctionListing {
    pub item: u64,
    pub buyout: u64,
    pub quantity: u64,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Region {
    US,
    EU,
}

pub struct BattleNetApiClient<'a> {
    pub token: String,
    client: Client,
    tt: ThreadThrottler,
    api_host: &'a str,
    api_locale: &'a str,
}

impl<'a> BattleNetApiClient<'a> {
    pub fn new(token: &str, region: Region) -> BattleNetApiClient {
        let mut hyper_client = Client::new();
        hyper_client.set_read_timeout(Some(Duration::from_secs(300)));

        BattleNetApiClient {
            token: token.to_owned(),
            client: hyper_client,
            tt: ThreadThrottler::new(100, Duration::new(1, 0)),
            api_host: match region {
                Region::US => "us.api.battle.net",
                Region::EU => "eu.api.battle.net",
            },
            api_locale: match region {
                Region::US => "en_US",
                Region::EU => "en_GB",
            },
        }
    }

    /// Try to retrieve something from the Blizzard API. Will retry indefinitely.
    /// Returns the body as a String.
    /// `task` will be used to generate error messages.
    fn make_blizzard_api_call(&self, url: &str, task: &str) -> String {
        let mut s = String::new();
        let mut retries = 0;

        loop {
            let mut res: Response;
            retries += 1;

            self.tt.pass_through_or_block();
            match self.client.get(url).send() {
                Ok(r) => res = r,
                Err(e) => {
                    println!("Error downloading {}: {}. Retry {}.", task, e, retries);
                    continue;
                },
            }
            // TODO: 404 should really be handled differently here. Maybe make this return a Result<T>?
            // That would let us account for unrecoverable errors.
            if res.status != hyper::Ok {
                println!("Error downloading {}: {}. Retry {}.", task, res.status, retries);
                continue;
            }
            match res.read_to_string(&mut s) {
                Ok(_) => (),
                Err(e) => {
                    println!("Failed to process {}: {}. Retry {}.", task, e, retries);
                    continue;
                },
            }
            return s;
        }
    }

    /// Downloads a list of realms from the Blizzard API.
    /// Panics if the json response is malformed.
    pub fn get_realms(&self) -> Vec<RealmInfo> {
        let mut realm_data: BTreeMap<String, Vec<RealmInfo>> = serde_json::from_str(&self.make_blizzard_api_call(
            &format!("https://{}/wow/realm/status?locale={}&apikey={}", self.api_host, self.api_locale, self.token), "realm status")
        ).unwrap();
        realm_data.remove("realms").expect("Malformed realm response.")
    }

    /// Downloads the auction listings for the specified realm, or None if the listings haven't
    /// been updated since `cutoff` or if the json response is illformed.
    pub fn get_auction_listings(&self, realm_slug: &str, cutoff: u64) -> Option<(u64, Vec<AuctionListing>)> {
        let mut auction_data_reply: AuctionDataReply;
        match serde_json::from_str(&self.make_blizzard_api_call(
            &format!("https://{}/wow/auction/data/{}?locale={}&apikey={}", self.api_host, realm_slug, self.api_locale, self.token),
            &format!("auction data for {}", realm_slug)))
        {
            Ok(reply) => auction_data_reply = reply,
            Err(e) => {
                println!("Bad json in auction pointer reply for {}: {}", realm_slug, e);
                return None;
            },
        }
        let auction_data_pointer = auction_data_reply.files.pop().unwrap();
        if auction_data_pointer.lastModified <= cutoff {
            return None;
        }

        let mut auction_data_str = self.make_blizzard_api_call(&auction_data_pointer.url, &format!("auction listings for {}", realm_slug));
        // Auction data strings are especially problematic and often contain numerous invalid bytes in the "owner" and
        // "ownerRealm" fields. Unfortunately, String::from_utf8_lossy() doesn't appear sufficient to deal with this
        // so we use the heavy handed approach of a regex to rewrite these fields.
        // TODO: Make this a lazy_static!.
        let sanitize_re = Regex::new("\"owner\":\".*?\",\"ownerRealm\":\".*?\",\"bid").unwrap();
        auction_data_str = String::from_utf8(
            sanitize_re.replace_all(auction_data_str.as_bytes(),
                                    &b"\"owner\":\"_\",\"ownerRealm\":\"_\",\"bid"[..])
            ).unwrap();
        match serde_json::from_str::<AuctionListingsReply>(&auction_data_str) {
            Ok(auction_listings_data) => Some((auction_data_pointer.lastModified, auction_listings_data.auctions)),
            Err(e) => {
                println!("Error decoding json auction listings for {}: {}", realm_slug, e);
                None
            },
        }
    }

    /// Helpler function to process a vec of RealmInfo's into vec's of slugs for
    /// connected realms. Connected realms share an auction house.
    pub fn process_connected_realms(realm_infos: &Vec<RealmInfo>) -> Vec<Vec<String>> {
        let mut realm_sets: Vec<Vec<String>> = realm_infos.into_iter().map(|r|
            r.connected_realms.clone()
        ).collect();

        // This dedup logic relies on the ordering within a connected realms list being the same
        // for all realms in the list.
        realm_sets.sort_by(|a, b| a.iter().next().unwrap().cmp(b.iter().next().unwrap()));
        realm_sets.dedup();
        return realm_sets;
    }

    /// Get info on an item. Panics on a malformed json response.
    pub fn get_item_info(&self, id: u64) -> ItemInfo {
        serde_json::from_str(&self.make_blizzard_api_call(
            &format!("https://{}/wow/item/{}?locale={}&apikey={}", self.api_host, id, self.api_locale, self.token), "item info")
        ).unwrap()
    }
}

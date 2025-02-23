use std::collections::HashMap;

use serde::{Serialize, Deserialize};
use warp::{Rejection, reply::Reply, reply::Response};

use super::errors::Error;
use super::handlers;
use super::storage;

#[derive(Deserialize, Serialize, Debug)]
pub struct RpcCall {
    pub endpoint: String,
    pub body: String,
    pub method: String,
    pub headers: String
}

#[derive(Debug, Deserialize)]
pub struct QueryOptions {
    pub limit: Option<u16>,
    pub from_server_id: Option<i64>
}

pub async fn handle_rpc_call(rpc_call: RpcCall) -> Result<Response, Rejection> {
    // Get a connection pool for the given room
    let room_id = match get_room_id(&rpc_call) {
        Some(room_id) => room_id,
        None => return Err(warp::reject::custom(Error::InvalidRpcCall))
    };
    let pool = storage::pool_by_room_id(room_id)?;
    // Check that the endpoint is a valid URI
    let uri = match rpc_call.endpoint.parse::<http::Uri>() {
        Ok(uri) => uri,
        Err(e) => {
            println!("Couldn't parse URI from: {} due to error: {}.", rpc_call.endpoint, e);
            return Err(warp::reject::custom(Error::InvalidRpcCall));
        }
    };
    // Get the auth token if possible
    let auth_token = get_auth_token(&rpc_call);
    // Switch on the HTTP method
    match rpc_call.method.as_ref() {
        "GET" => return handle_get_request(rpc_call, uri, &pool).await,
        "POST" => return handle_post_request(rpc_call, uri, auth_token, &pool).await,
        "DELETE" => return handle_delete_request(rpc_call, uri, auth_token, &pool).await,
        _ => {
            println!("Ignoring RPC call with invalid or unused HTTP method: {}.", rpc_call.method);
            return Err(warp::reject::custom(Error::InvalidRpcCall));
        }
    }
}

async fn handle_get_request(rpc_call: RpcCall, uri: http::Uri, pool: &storage::DatabaseConnectionPool) -> Result<Response, Rejection> {
    // Switch on the path
    if uri.path().starts_with("/files") {
        let components: Vec<&str> = uri.path()[1..].split("/").collect(); // Drop the leading slash and split on subsequent slashes
        if components.len() != 2 {
            println!("Invalid endpoint: {}.", rpc_call.endpoint);
            return Err(warp::reject::custom(Error::InvalidRpcCall));
        }
        let file_id = components[1];
        return handlers::get_file(file_id).await.map(|json| warp::reply::json(&json).into_response());
    }
    match uri.path() {
        "/messages" => {
            let query_options: QueryOptions;
            if let Some(query) = uri.query() {
                query_options = match serde_json::from_str(&query) {
                    Ok(query_options) => query_options,
                    Err(e) => {
                        println!("Couldn't parse query options from: {} due to error: {}.", query, e);
                        return Err(warp::reject::custom(Error::InvalidRpcCall));
                    }
                };
            } else {
                println!("Missing query options.");
                return Err(warp::reject::custom(Error::InvalidRpcCall));
            }
            return handlers::get_messages(query_options, pool).await;
        },
        "/deleted_messages" => {
            let query_options: QueryOptions;
            if let Some(query) = uri.query() {
                query_options = match serde_json::from_str(&query) {
                    Ok(query_options) => query_options,
                    Err(e) => {
                        println!("Couldn't parse query options from: {} due to error: {}.", query, e);
                        return Err(warp::reject::custom(Error::InvalidRpcCall));
                    }
                };
            } else {
                println!("Missing query options.");
                return Err(warp::reject::custom(Error::InvalidRpcCall));
            }
            return handlers::get_deleted_messages(query_options, pool).await
        },
        "/moderators" => return handlers::get_moderators(pool).await,
        "/block_list" => return handlers::get_banned_public_keys(pool).await,
        "/member_count" => return handlers::get_member_count(pool).await,
        "/auth_token_challenge" => {
            #[derive(Debug, Deserialize)]
            struct QueryOptions { public_key: String }
            let query_options: QueryOptions;
            if let Some(query) = uri.query() {
                query_options = match serde_json::from_str(&query) {
                    Ok(query_options) => query_options,
                    Err(e) => {
                        println!("Couldn't parse query options from: {} due to error: {}.", query, e);
                        return Err(warp::reject::custom(Error::InvalidRpcCall));
                    }
                };
            } else {
                println!("Missing query options.");
                return Err(warp::reject::custom(Error::InvalidRpcCall));
            }
            return handlers::get_auth_token_challenge(&query_options.public_key, pool).await.map(|json| warp::reply::json(&json).into_response());
        },
        _ => {
            println!("Ignoring RPC call with invalid or unused endpoint: {}.", rpc_call.endpoint);
            return Err(warp::reject::custom(Error::InvalidRpcCall));        
        }
    }
}

async fn handle_post_request(rpc_call: RpcCall, uri: http::Uri, auth_token: Option<String>, pool: &storage::DatabaseConnectionPool) -> Result<Response, Rejection> {
    match uri.path() {
        "/messages" => {
            let message = match serde_json::from_str(&rpc_call.body) {
                Ok(message) => message,
                Err(e) => {
                    println!("Couldn't parse message from: {} due to error: {}.", rpc_call.body, e);
                    return Err(warp::reject::custom(Error::InvalidRpcCall));
                }
            };
            return handlers::insert_message(message, auth_token, pool).await; 
        },
        "/block_list" => {
            #[derive(Debug, Deserialize)]
            struct JSON { public_key: String }
            let json: JSON = match serde_json::from_str(&rpc_call.body) {
                Ok(message) => message,
                Err(e) => {
                    println!("Couldn't parse JSON from: {} due to error: {}.", rpc_call.body, e);
                    return Err(warp::reject::custom(Error::InvalidRpcCall));
                }
            };
            return handlers::ban(&json.public_key, auth_token, pool).await;
        },
        "/claim_auth_token" => {
            #[derive(Debug, Deserialize)]
            struct JSON { public_key: String }
            let json: JSON = match serde_json::from_str(&rpc_call.body) {
                Ok(message) => message,
                Err(e) => {
                    println!("Couldn't parse JSON from: {} due to error: {}.", rpc_call.body, e);
                    return Err(warp::reject::custom(Error::InvalidRpcCall));
                }
            };
            return handlers::claim_auth_token(&json.public_key, auth_token, pool).await;
        },
        "/files" => {
            #[derive(Debug, Deserialize)]
            struct JSON { file: String }
            let json: JSON = match serde_json::from_str(&rpc_call.body) {
                Ok(message) => message,
                Err(e) => {
                    println!("Couldn't parse JSON from: {} due to error: {}.", rpc_call.body, e);
                    return Err(warp::reject::custom(Error::InvalidRpcCall));
                }
            };
            return handlers::store_file(&json.file, pool).await;
        },
        _ => {
            println!("Ignoring RPC call with invalid or unused endpoint: {}.", rpc_call.endpoint);
            return Err(warp::reject::custom(Error::InvalidRpcCall));        
        }
    }
}

async fn handle_delete_request(rpc_call: RpcCall, uri: http::Uri, auth_token: Option<String>, pool: &storage::DatabaseConnectionPool) -> Result<Response, Rejection> {
    // DELETE /messages/:server_id
    if uri.path().starts_with("/messages") {
        let components: Vec<&str> = uri.path()[1..].split("/").collect(); // Drop the leading slash and split on subsequent slashes
        if components.len() != 2 {
            println!("Invalid endpoint: {}.", rpc_call.endpoint);
            return Err(warp::reject::custom(Error::InvalidRpcCall));
        }
        let server_id: i64 = match components[1].parse() {
            Ok(server_id) => server_id,
            Err(_) => {
                println!("Invalid endpoint: {}.", rpc_call.endpoint);
                return Err(warp::reject::custom(Error::InvalidRpcCall));
            }
        };
        return handlers::delete_message(server_id, auth_token, pool).await;
    }
    // DELETE /block_list/:public_key
    if uri.path().starts_with("/block_list") {
        let components: Vec<&str> = uri.path()[1..].split("/").collect(); // Drop the leading slash and split on subsequent slashes
        if components.len() != 2 {
            println!("Invalid endpoint: {}.", rpc_call.endpoint);
            return Err(warp::reject::custom(Error::InvalidRpcCall));
        }
        let public_key = components[1].to_string();
        return handlers::unban(&public_key, auth_token, pool).await;
    }
    // DELETE /auth_token
    if uri.path() == "/auth_token" {
        return handlers::delete_auth_token(auth_token, pool).await;
    }
    // Unrecognized endpoint
    println!("Ignoring RPC call with invalid or unused endpoint: {}.", rpc_call.endpoint);
    return Err(warp::reject::custom(Error::InvalidRpcCall));
}

// Utilities

fn get_auth_token(rpc_call: &RpcCall) -> Option<String> {
    if rpc_call.headers.is_empty() { return None; }
    let headers: HashMap<String, String> = match serde_json::from_str(&rpc_call.headers) {
        Ok(headers) => headers,
        Err(_) => return None
    };
    return headers.get("Authorization").map(|s| s.to_string());
}

fn get_room_id(rpc_call: &RpcCall) -> Option<isize> {
    if rpc_call.headers.is_empty() { return None; }
    let headers: HashMap<String, String> = match serde_json::from_str(&rpc_call.headers) {
        Ok(headers) => headers,
        Err(_) => return None
    };
    let header = headers.get("Room")?;
    match header.parse() {
        Ok(room_id) => return Some(room_id),
        Err(_) => return None
    };
}
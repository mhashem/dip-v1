use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use dip_v1::catalog::catalog_manager::CatalogManager;
use dip_v1::sql::engine::SQLEngine;
use dip_v1::storage::buffer_pool_manager::BufferPoolManager;
use dip_v1::storage::disk_manager::DiskManager;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct AppState {
    engine: Arc<Mutex<SQLEngine>>,
}

#[derive(Deserialize)]
struct SensorData {
    sensor_id: i32,
    value: i32,
}

#[derive(Serialize)]
struct ReadingResponse {
    sensor_id: i32,
    value: i32,
}

#[tokio::main]
async fn main() {
    // Initialize Database
    let path = std::env::current_dir().unwrap().join("sensor.db");
    let dm = DiskManager::new(&path).unwrap();
    let bpm = Arc::new(BufferPoolManager::new(100, dm));
    let catalog = CatalogManager::new(bpm);
    let mut engine = SQLEngine::new(catalog);

    // Init Table
    let _ = engine.execute("CREATE TABLE SensorReadings (sensor_id INT, value INT)");

    let state = AppState {
        engine: Arc::new(Mutex::new(engine)),
    };

    let app = Router::new()
        .route("/api/v1/sense", post(record_sense))
        .route("/api/v1/readings", get(get_readings))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    println!("Listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn record_sense(
    State(state): State<AppState>,
    Json(payload): Json<SensorData>,
) -> Json<String> {
    let mut engine = state.engine.lock().unwrap();
    let sql = format!(
        "INSERT INTO SensorReadings VALUES ({}, {})",
        payload.sensor_id,
        payload.value
    );
    
    match engine.execute(&sql) {
        Ok(_) => Json("Recorded".to_string()),
        Err(e) => Json(format!("Error: {}", e)),
    }
}

async fn get_readings(State(state): State<AppState>) -> Json<Vec<ReadingResponse>> {
    let mut engine = state.engine.lock().unwrap();
    let output = engine.execute("SELECT * FROM SensorReadings").unwrap();
    
    // Parse output string (Temporary hack because our engine returns String)
    // In a real usage, we'd want execute() to return an Iterator or ResultSet.
    // Format is: "| 101 | 50 |\n..."
    
    let mut readings = Vec::new();
    for line in output.lines() {
        if line.trim().is_empty() || line.contains("sensor_id") || line.contains("---") {
            continue;
        }
        
        let parts: Vec<&str> = line.split('|').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        if parts.len() >= 2 {
            if let (Ok(id), Ok(val)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
                readings.push(ReadingResponse {
                    sensor_id: id,
                    value: val,
                });
            }
        }
    }

    Json(readings)
}
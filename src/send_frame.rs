use axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
};
use std::io::Cursor;
use std::sync::{Arc, Mutex};
// 允许提取连接用户的IP
use crate::app_state::SharedState;
use crate::process_frame::process_frame_with_candle;
use axum::extract::State;
use axum::http::StatusCode;
use opencv::core::MatTraitConst;
use opencv::videoio;
use opencv::videoio::{VideoCapture, VideoCaptureTrait};

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    let opt_camera = state.read().unwrap().camera.clone();
    match opt_camera {
        Some(camera) => ws.on_upgrade(move |socket| handle_socket(socket, camera)),
        None => (StatusCode::INTERNAL_SERVER_ERROR, "摄像头未打开").into_response(),
    }
}

async fn handle_socket(mut socket: WebSocket, camera: Arc<Mutex<VideoCapture>>) {
    loop {
        let frame_data = {
            let mut locked_camera = camera.lock().unwrap();
            let mut frame = opencv::core::Mat::default();
            let device = match crate::pose::utils::device(false) {
                Ok(device) => device,
                Err(e) => {
                    tracing::error!("加载模型失败: {}", e);
                    break;
                }
            };
            match locked_camera.read(&mut frame) {
                Ok(_) => {
                    // let mut buf = Default::default();
                    // imgcodecs::imencode(".jpeg", &frame, &mut buf, &Default::default()).ok();
                    // Some(Vec::from(buf))
                    // 使用 candle 处理图像
                    let size = match frame.size() {
                        Ok(size) => size,
                        Err(e) => {
                            tracing::error!("读取图片失败: {}", e);
                            break;
                        }
                    };
                    if size.width > 0 {
                        let mut buf = Vec::new();
                        let mut cursor = Cursor::new(&mut buf);
                        let processed_frame =
                            process_frame_with_candle(&frame, &device, None, None, None).unwrap();
                        let processed_frame = processed_frame.to_rgb8();
                        processed_frame
                            .write_to(&mut cursor, image::ImageOutputFormat::Jpeg(90))
                            .unwrap();
                        Some(buf)
                    } else {
                        tracing::error!("读取图片结束");
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("读取图片失败: {}", e);
                    break;
                }
            }
        };

        if let Some(buf) = frame_data {
            match socket.send(Message::Binary(buf)).await {
                Ok(_) => {}
                Err(e) => {
                    tracing::error!("发送图片失败: {}", e);
                    break;
                }
            }
        }
    }
}

pub async fn start_camera(State(state): State<SharedState>) -> String {
    // 从全局状态中获取相机，如果相机不存在，则创建一个新的相机，如果相机存在，就先关闭相机，再开启一个新的相机

    let mut app_state = state.write().unwrap();
    let opt_camera = app_state.camera.take();

    if opt_camera.is_none() {
        let camera = VideoCapture::new(0, videoio::CAP_ANY);
        match camera {
            Ok(camera) => {
                let shared_camera = Arc::new(Mutex::new(camera));
                app_state.camera = Some(shared_camera);
            }
            Err(e) => {
                return format!("打开相机失败: {}", e);
            }
        }
        "打开相机成功".to_string()
    } else {
        // 检查相机是否已打开
        "相机已经打开".to_string()
    }
}

/// 从全局状态中获取相机，如果相机不存在，则返回不存打开的相机，如果相机存在，就先关闭相机，再返回已经关闭相机
pub async fn stop_camera(State(state): State<SharedState>) -> String {
    let mut app_state = state.write().unwrap();
    let opt_camera = app_state.camera.take();
    match opt_camera {
        Some(camera) => {
            let mut locked_camera = match camera.lock() {
                Ok(camera) => camera,
                Err(e) => {
                    return format!("锁定相机失败: {}", e);
                }
            };
            match locked_camera.release() {
                Ok(_) => "相机关闭成功".to_string(),
                Err(e) => {
                    format!("关闭相机失败: {}", e)
                }
            }
        }
        None => "相机已经关闭".to_string(),
    }
}

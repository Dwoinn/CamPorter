// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
use sysinfo::Disks;
use serde::Serialize;
use std::process::Command;
use std::fs;
use std::path::Path;
use walkdir::WalkDir;
use tauri::{Emitter, Manager};
use base64::{engine::general_purpose, Engine as _};
use image::{ImageFormat};
use tempfile::tempdir;
use which::which;

#[cfg(not(target_os = "windows"))]
use thumbnails::Thumbnailer;

#[derive(Serialize)]
struct RemovableDrive {
    name: String,
    mount_point: String,
    device_id: String,
}

#[derive(Serialize)]
struct MediaFile {
    name: String,
    path: String,
    size: u64,
    modified: u64, // Unix timestamp
    extension: String,
    is_image: bool,
    is_video: bool,
}

#[tauri::command]
fn list_removable_drives() -> Vec<RemovableDrive> {
    let disks = Disks::new_with_refreshed_list();
    
    disks.iter()
        .filter(|disk| {
            let mount_point = disk.mount_point().to_string_lossy();
            let name = disk.name().to_string_lossy();
            
            // On Linux, removable drives often mount under /media, /mnt, or /run/media
            // Also check for common USB drive characteristics
            disk.is_removable() ||
            mount_point.starts_with("/media/") ||
            mount_point.starts_with("/mnt/") ||
            mount_point.starts_with("/run/media/") ||
            name.contains("sd") ||  // SD cards
            name.contains("usb") || // USB drives
            name.contains("removable")
        })
        .map(|disk| RemovableDrive {
            name: format!("{} ({})",
                disk.name().to_string_lossy(),
                if disk.is_removable() { "removable" } else { "mounted" }
            ),
            mount_point: disk.mount_point().to_string_lossy().to_string(),
            device_id: disk.name().to_string_lossy().to_string(),
        })
        .collect()
}

#[tauri::command]
fn list_media_files(drive_path: String) -> Result<Vec<MediaFile>, String> {
    let src = Path::new(&drive_path);
    
    if !src.exists() {
        return Err("Drive path does not exist".to_string());
    }
    
    let media_extensions = ["mp4", "jpg", "jpeg", "png", "mov", "heic", "mp3", "wav", "avi", "mkv", "gif"];
    let image_extensions = ["jpg", "jpeg", "png", "heic", "gif"];
    let video_extensions = ["mp4", "mov", "avi", "mkv"];
    
    let mut media_files = Vec::new();
    
    for entry in WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if media_extensions.contains(&ext_lower.as_str()) {
                    if let Ok(metadata) = entry.metadata() {
                        let modified = metadata
                            .modified()
                            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                            .duration_since(std::time::SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        
                        let file_name = entry.file_name().to_string_lossy().to_string();
                        let is_image = image_extensions.contains(&ext_lower.as_str());
                        let is_video = video_extensions.contains(&ext_lower.as_str());
                        
                        media_files.push(MediaFile {
                            name: file_name,
                            path: entry.path().to_string_lossy().to_string(),
                            size: metadata.len(),
                            modified,
                            extension: ext_lower,
                            is_image,
                            is_video,
                        });
                    }
                }
            }
        }
    }
    
    // Sort by modification time (newest first)
    media_files.sort_by(|a, b| b.modified.cmp(&a.modified));
    
    Ok(media_files)
}

#[tauri::command]
async fn unmount_drive(mount_point: String) -> Result<(), String> {
    let mut command = if cfg!(target_os = "linux") {
        let mut cmd = Command::new("udisksctl");
        cmd.args(["unmount", "-p", &mount_point]);
        cmd
    } else if cfg!(target_os = "macos") {
        let mut cmd = Command::new("diskutil");
        cmd.args(["unmount", &mount_point]);
        cmd
    } else if cfg!(target_os = "windows") {
        let mut cmd = Command::new("powershell");
        cmd.args([
            "-Command",
            &format!("(New-Object -comObject Shell.Application).Namespace(17).ParseName('{}').InvokeVerb('Eject')", mount_point),
        ]);
        cmd
    } else {
        return Err("Unsupported platform".to_string());
    };

    let output = command.output().map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        Err(format!("Unmount failed: {} {}", stderr, stdout))
    }
}

#[tauri::command]
async fn import_selected_files(
    file_paths: Vec<String>,
    target_path: String,
    window: tauri::Window,
) -> Result<(), String> {
    let dest = Path::new(&target_path);
    
    // Create target directory if it doesn't exist
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    
    // Calculate total size of all files
    let mut total_size: u64 = 0;
    let mut file_sizes: Vec<u64> = Vec::new();
    
    for file_path in &file_paths {
        let src_file = Path::new(file_path);
        if src_file.exists() {
            if let Ok(metadata) = src_file.metadata() {
                let size = metadata.len();
                total_size += size;
                file_sizes.push(size);
            } else {
                file_sizes.push(0);
            }
        } else {
            file_sizes.push(0);
        }
    }
    
    let mut copied_size: u64 = 0;
    
    for (i, file_path) in file_paths.iter().enumerate() {
        let src_file = Path::new(file_path);
        let file_size = file_sizes[i];
        
        if !src_file.exists() {
            window.emit("import-progress", &format!("Skipped: {} (file not found)", file_path)).map_err(|e| e.to_string())?;
            continue;
        }
        
        let file_name = src_file.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
        let target_file = dest.join(file_name);
        
        // Skip if file already exists
        if target_file.exists() {
            window.emit("import-progress", &format!("Skipped: {} (already exists)", file_name)).map_err(|e| e.to_string())?;
            copied_size += file_size; // Count as "copied" for progress calculation
            window.emit("import-progress", &format!("PROGRESS_BYTES:{}:{}", copied_size, total_size)).map_err(|e| e.to_string())?;
            continue;
        }
        
        window.emit("import-progress", &format!("Copying: {}", file_name)).map_err(|e| e.to_string())?;
        
        // Copy file with progress tracking for large files
        match copy_file_with_progress(src_file, &target_file, file_size, copied_size, total_size, &window) {
            Ok(_) => {
                copied_size += file_size;
                window.emit("import-progress", &format!("Copied: {}", file_name)).map_err(|e| e.to_string())?;
            }
            Err(e) => {
                window.emit("import-progress", &format!("Failed to copy {}: {}", file_name, e)).map_err(|e| e.to_string())?;
            }
        }
        
        // Report final progress for this file
        window.emit("import-progress", &format!("PROGRESS_BYTES:{}:{}", copied_size, total_size)).map_err(|e| e.to_string())?;
    }
    
    Ok(())
}

fn copy_file_with_progress(
    src: &Path,
    dest: &Path,
    file_size: u64,
    initial_copied: u64,
    total_size: u64,
    window: &tauri::Window,
) -> Result<u64, std::io::Error> {
    use std::io::{Read, Write};
    
    let mut src_file = fs::File::open(src)?;
    let mut dest_file = fs::File::create(dest)?;
    
    let mut buffer = [0; 64 * 1024]; // 64KB buffer
    let mut copied_this_file = 0u64;
    let mut last_progress_report = 0u64;
    
    loop {
        let bytes_read = src_file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        
        dest_file.write_all(&buffer[..bytes_read])?;
        copied_this_file += bytes_read as u64;
        
        // Report progress every 1MB or 10% of file, whichever is smaller
        let progress_interval = std::cmp::min(1024 * 1024, file_size / 10).max(64 * 1024);
        
        if copied_this_file - last_progress_report >= progress_interval || copied_this_file == file_size {
            let total_copied = initial_copied + copied_this_file;
            if let Err(_) = window.emit("import-progress", &format!("PROGRESS_BYTES:{}:{}", total_copied, total_size)) {
                // Continue even if progress reporting fails
            }
            last_progress_report = copied_this_file;
        }
    }
    
    dest_file.sync_all()?;
    Ok(copied_this_file)
}

#[tauri::command]
async fn import_media(
    source_path: String,
    target_path: String,
    window: tauri::Window,
) -> Result<(), String> {
    let src = Path::new(&source_path);
    let dest = Path::new(&target_path);
    
    // Create target directory if it doesn't exist
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    
    // Collect all media files recursively
    let media_extensions = ["mp4", "jpg", "jpeg", "png", "mov", "heic"];
    let mut media_files = Vec::new();
    
    for entry in WalkDir::new(src).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension().and_then(|e| e.to_str()) {
                if media_extensions.contains(&ext.to_lowercase().as_str()) {
                    media_files.push(entry.path().to_owned());
                }
            }
        }
    }
    
    let total = media_files.len();
    for (i, file) in media_files.iter().enumerate() {
        let file_name = file.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
        let target_file = dest.join(file_name);
        
        // Skip if file already exists
        if target_file.exists() {
            continue;
        }
        
        // Copy file
        fs::copy(file, &target_file).map_err(|e| e.to_string())?;
        
        // Report progress
        window.emit("import-progress", &format!("Copied: {}", file_name)).map_err(|e| e.to_string())?;
        window.emit("import-progress", &format!("PROGRESS:{}:{}", i+1, total)).map_err(|e| e.to_string())?;
    }
    
    Ok(())
}

/// Check if FFmpeg is installed on the system
fn is_ffmpeg_available() -> bool {
    // Try to find ffmpeg in PATH
    println!("Checking if FFmpeg is available...");
    let result = which("ffmpeg");
    match &result {
        Ok(path) => println!("FFmpeg found at: {}", path.display()),
        Err(e) => println!("FFmpeg not found: {}", e),
    }
    result.is_ok()
}

/// Generate a video thumbnail using FFmpeg
fn generate_video_thumbnail(video_path: &Path) -> Result<String, String> {
    println!("Generating thumbnail for video: {}", video_path.display());
    
    // Create a temporary directory for the thumbnail
    let temp_dir = tempdir().map_err(|e| format!("Failed to create temp dir: {}", e))?;
    let output_path = temp_dir.path().join("thumbnail.png");
    
    println!("Temp output path: {}", output_path.display());
    
    // Get the absolute path to the video file
    let video_absolute_path = fs::canonicalize(video_path)
        .map_err(|e| format!("Failed to get absolute path: {}", e))?;
    
    println!("Video absolute path: {}", video_absolute_path.display());
    
    // Build FFmpeg command to extract a frame from the video
    let ffmpeg_cmd = if cfg!(target_os = "windows") {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    
    println!("Using FFmpeg command: {}", ffmpeg_cmd);
    
    let mut command = Command::new(ffmpeg_cmd);
    
    // Add arguments
    command
        .arg("-i").arg(&video_absolute_path)
        .arg("-ss").arg("00:00:01") // Take frame at 1 second
        .arg("-vframes").arg("1")
        .arg("-vf").arg("scale=250:-1") // Scale to 250px width, maintain aspect ratio
        .arg("-y") // Overwrite output file if it exists
        .arg(&output_path);
    
    // Print the command for debugging
    let cmd_str = format!("{:?}", command);
    println!("FFmpeg command: {}", cmd_str);
    
    // Execute FFmpeg command
    let output = command.output().map_err(|e| format!("Failed to execute FFmpeg: {}", e))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        println!("FFmpeg stdout: {}", stdout);
        println!("FFmpeg stderr: {}", stderr);
        return Err(format!("FFmpeg error: {}", stderr));
    }
    
    println!("FFmpeg executed successfully, checking if output file exists");
    
    // Check if the output file exists
    if !output_path.exists() {
        return Err(format!("Output file not created: {}", output_path.display()));
    }
    
    println!("Output file exists, reading file");
    
    // Read the file to bytes and encode to base64
    let img_data = fs::read(&output_path).map_err(|e| format!("Failed to read thumbnail: {}", e))?;
    
    println!("Read {} bytes from output file", img_data.len());
    
    let res_base64 = general_purpose::STANDARD.encode(&img_data);
    
    // Determine MIME type
    let mime_type = "image/png";
    
    println!("Successfully generated thumbnail");
    
    Ok(format!("data:{};base64,{}", mime_type, res_base64))
}

#[tauri::command]
fn get_file_thumbnail(file_path: String) -> Result<String, String> {
    println!("Getting thumbnail for file: {}", file_path);
    
    let path = Path::new(&file_path);
    
    if !path.exists() {
        println!("File does not exist: {}", file_path);
        return Err("File does not exist".to_string());
    }
    
    let is_video = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => {
            let ext_lower = ext.to_lowercase();
            println!("File extension: {}", ext_lower);
            matches!(ext_lower.as_str(), "mp4" | "mov" | "avi" | "mkv")
        },
        None => {
            println!("No file extension found");
            false
        },
    };
    
    if is_video {
        println!("File is a video, attempting to generate thumbnail");
        
        // For videos, try to use FFmpeg first if available
        if is_ffmpeg_available() {
            println!("FFmpeg is available, using it to generate thumbnail");
            match generate_video_thumbnail(path) {
                Ok(data_url) => {
                    println!("Successfully generated thumbnail with FFmpeg");
                    return Ok(data_url);
                },
                Err(e) => {
                    println!("Failed to generate thumbnail with FFmpeg: {}", e);
                    println!("Falling back to alternative methods");
                    
                    // Fall back to platform-specific methods if FFmpeg fails
                    #[cfg(not(target_os = "windows"))]
                    {
                        println!("Using Thumbnailer on non-Windows platform");
                        let thumbnailer = Thumbnailer::new(250, 250);
                        match thumbnailer.get(path) {
                            Ok(img) => {
                                println!("Thumbnailer succeeded, converting to base64");
                                let mut buf = Vec::new();
                                let mut cursor = std::io::Cursor::new(&mut buf);
                                
                                if let Err(e) = img.write_to(&mut cursor, ImageFormat::Png) {
                                    println!("Failed to write thumbnail to buffer: {}", e);
                                    return generate_fallback_thumbnail(path);
                                }
                                
                                let res_base64 = general_purpose::STANDARD.encode(&buf);
                                let mime_type = get_mime_type(path);
                                
                                println!("Successfully generated thumbnail with Thumbnailer");
                                return Ok(format!("data:{};base64,{}", mime_type, res_base64));
                            },
                            Err(e) => {
                                println!("Thumbnailer failed: {}", e);
                                println!("Using fallback thumbnail");
                                return generate_fallback_thumbnail(path);
                            },
                        }
                    }
                    
                    #[cfg(target_os = "windows")]
                    {
                        println!("On Windows, using fallback thumbnail");
                        return generate_fallback_thumbnail(path);
                    }
                }
            }
        } else {
            println!("FFmpeg is not available");
            
            // If FFmpeg is not available, fall back to platform-specific methods
            #[cfg(not(target_os = "windows"))]
            {
                println!("Using Thumbnailer on non-Windows platform");
                let thumbnailer = Thumbnailer::new(250, 250);
                match thumbnailer.get(path) {
                    Ok(img) => {
                        println!("Thumbnailer succeeded, converting to base64");
                        let mut buf = Vec::new();
                        let mut cursor = std::io::Cursor::new(&mut buf);
                        
                        if let Err(e) = img.write_to(&mut cursor, ImageFormat::Png) {
                            println!("Failed to write thumbnail to buffer: {}", e);
                            return generate_fallback_thumbnail(path);
                        }
                        
                        let res_base64 = general_purpose::STANDARD.encode(&buf);
                        let mime_type = get_mime_type(path);
                        
                        println!("Successfully generated thumbnail with Thumbnailer");
                        return Ok(format!("data:{};base64,{}", mime_type, res_base64));
                    },
                    Err(e) => {
                        println!("Thumbnailer failed: {}", e);
                        println!("Using fallback thumbnail");
                        return generate_fallback_thumbnail(path);
                    },
                }
            }
            
            #[cfg(target_os = "windows")]
            {
                println!("On Windows, using fallback thumbnail");
                return generate_fallback_thumbnail(path);
            }
        }
    } else {
        println!("File is an image, using image crate");
        
        // For images, use the image crate directly
        match image::open(path) {
            Ok(img) => {
                println!("Successfully opened image, creating thumbnail");
                let thumbnail = img.thumbnail(250, 250);
                
                let mut buf = Vec::new();
                let mut cursor = std::io::Cursor::new(&mut buf);
                
                if let Err(e) = thumbnail.write_to(&mut cursor, ImageFormat::Png) {
                    println!("Failed to write thumbnail to buffer: {}", e);
                    return generate_fallback_thumbnail(path);
                }
                
                let res_base64 = general_purpose::STANDARD.encode(&buf);
                let mime_type = get_mime_type(path);
                
                println!("Successfully generated thumbnail for image");
                return Ok(format!("data:{};base64,{}", mime_type, res_base64));
            },
            Err(e) => {
                println!("Failed to open image: {}", e);
                println!("Using fallback thumbnail");
                return generate_fallback_thumbnail(path);
            },
        }
    }
}

fn generate_fallback_thumbnail(path: &Path) -> Result<String, String> {
    println!("Generating fallback thumbnail for: {}", path.display());
    
    // Create a simple colored rectangle based on file type
    let width = 250;
    let height = 250;
    let mut img = image::RgbaImage::new(width, height);
    
    // Fill with a color based on file extension
    let color = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => {
            let ext_lower = ext.to_lowercase();
            match ext_lower.as_str() {
                "mp4" | "mov" | "avi" | "mkv" => [120, 120, 255, 255], // Blue for video
                "jpg" | "jpeg" | "png" | "gif" => [120, 255, 120, 255], // Green for images
                _ => [200, 200, 200, 255], // Gray for other types
            }
        },
        None => [200, 200, 200, 255], // Gray for unknown types
    };
    
    println!("Using color: {:?}", color);
    
    // Draw a video icon in the center for video files
    let is_video = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => {
            let ext_lower = ext.to_lowercase();
            matches!(ext_lower.as_str(), "mp4" | "mov" | "avi" | "mkv")
        },
        None => false,
    };
    
    // Fill the background
    for pixel in img.pixels_mut() {
        *pixel = image::Rgba(color);
    }
    
    // If it's a video, add a play icon indicator
    if is_video {
        println!("Adding play icon indicator for video");
        
        // Draw a simple play icon (white triangle) in the center
        let center_x = width / 2;
        let center_y = height / 2;
        
        // Draw a white circle in the center
        let radius = 40;
        for y in 0..height {
            for x in 0..width {
                let dx = x as i32 - center_x as i32;
                let dy = y as i32 - center_y as i32;
                let distance = (dx * dx + dy * dy) as f32;
                
                // Circle outline
                if distance.sqrt() <= radius as f32 && distance.sqrt() >= (radius - 5) as f32 {
                    img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
                }
                
                // Play triangle
                if distance.sqrt() < radius as f32 - 10.0 {
                    // Simple right-pointing triangle
                    if x > center_x - 15 && x < center_x + 15 &&
                       y > center_y - 15 && y < center_y + 15 {
                        if x > center_x - 10 &&
                           y > center_y - 10 - (x - center_x) / 2 &&
                           y < center_y + 10 - (x - center_x) / 2 {
                            img.put_pixel(x, y, image::Rgba([255, 255, 255, 255]));
                        }
                    }
                }
            }
        }
    }
    
    let mut buf = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut buf);
    
    println!("Writing fallback thumbnail to buffer");
    
    match image::DynamicImage::ImageRgba8(img).write_to(&mut cursor, ImageFormat::Png) {
        Ok(_) => {
            println!("Successfully wrote fallback thumbnail to buffer");
            let res_base64 = general_purpose::STANDARD.encode(&buf);
            
            // Determine MIME type based on extension
            let mime_type = get_mime_type(path);
            
            println!("Fallback thumbnail generated successfully");
            Ok(format!("data:{};base64,{}", mime_type, res_base64))
        },
        Err(e) => {
            println!("Failed to write fallback thumbnail: {}", e);
            Err(format!("Error writing PNG: {e}"))
        }
    }
}

fn get_mime_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("heic") => "image/heic",
        Some("mp4") => "video/mp4",
        Some("mov") => "video/quicktime",
        Some("avi") => "video/x-msvideo",
        Some("mkv") => "video/x-matroska",
        _ => "application/octet-stream",
    }
}

#[tauri::command]
async fn save_destination_path(path: String, app: tauri::AppHandle) -> Result<(), String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    
    // Create app data directory if it doesn't exist
    if !app_data_dir.exists() {
        fs::create_dir_all(&app_data_dir).map_err(|e| e.to_string())?;
    }
    
    let config_file = app_data_dir.join("config.json");
    let config = serde_json::json!({
        "destination_path": path
    });
    
    fs::write(config_file, config.to_string()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn load_destination_path(app: tauri::AppHandle) -> Result<String, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let config_file = app_data_dir.join("config.json");
    
    if !config_file.exists() {
        return Ok(String::new());
    }
    
    let config_content = fs::read_to_string(config_file).map_err(|e| e.to_string())?;
    let config: serde_json::Value = serde_json::from_str(&config_content).map_err(|e| e.to_string())?;
    
    Ok(config["destination_path"].as_str().unwrap_or("").to_string())
}

#[tauri::command]
async fn open_destination_folder(path: String) -> Result<(), String> {
    let dest_path = Path::new(&path);
    
    if !dest_path.exists() {
        return Err("Destination folder does not exist".to_string());
    }
    
    let mut command = if cfg!(target_os = "linux") {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(&path);
        cmd
    } else if cfg!(target_os = "macos") {
        let mut cmd = Command::new("open");
        cmd.arg(&path);
        cmd
    } else if cfg!(target_os = "windows") {
        let mut cmd = Command::new("explorer");
        cmd.arg(&path);
        cmd
    } else {
        return Err("Unsupported platform".to_string());
    };

    command.output().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn check_files_exist_in_destination(
    file_paths: Vec<String>,
    destination_path: String,
) -> Result<Vec<bool>, String> {
    let dest = Path::new(&destination_path);
    let mut results = Vec::new();
    
    for file_path in file_paths {
        let src_file = Path::new(&file_path);
        let file_name = src_file.file_name().and_then(|n| n.to_str()).unwrap_or("unknown");
        let target_file = dest.join(file_name);
        results.push(target_file.exists());
    }
    
    Ok(results)
}

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            greet,
            list_removable_drives,
            list_media_files,
            get_file_thumbnail,
            unmount_drive,
            import_selected_files,
            import_media,
            save_destination_path,
            load_destination_path,
            open_destination_folder,
            check_files_exist_in_destination
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

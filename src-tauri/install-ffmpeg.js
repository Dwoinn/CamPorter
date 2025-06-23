import { execSync } from 'child_process';
import os from 'os';
import fs from 'fs';
import path from 'path';

// Determine the platform
const platform = os.platform();

console.log(`Detected platform: ${platform}`);

try {
  // Check if FFmpeg is already installed
  try {
    const ffmpegVersion = execSync('ffmpeg -version').toString();
    console.log('FFmpeg is already installed:');
    console.log(ffmpegVersion.split('\n')[0]);
    process.exit(0);
  } catch (error) {
    console.log('FFmpeg is not installed or not in PATH. Installing...');
  }

  // Install FFmpeg based on the platform
  if (platform === 'win32') {
    // Windows installation
    console.log('Installing FFmpeg on Windows...');
    execSync('powershell -Command "Set-ExecutionPolicy Bypass -Scope Process -Force; [System.Net.ServicePointManager]::SecurityProtocol = [System.Net.ServicePointManager]::SecurityProtocol -bor 3072; iex ((New-Object System.Net.WebClient).DownloadString(\'https://chocolatey.org/install.ps1\'))"', { stdio: 'inherit' });
    execSync('choco install ffmpeg -y', { stdio: 'inherit' });
  } else if (platform === 'darwin') {
    // macOS installation
    console.log('Installing FFmpeg on macOS...');
    try {
      execSync('brew --version', { stdio: 'ignore' });
    } catch (error) {
      console.log('Homebrew not installed. Installing Homebrew...');
      execSync('/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"', { stdio: 'inherit' });
    }
    execSync('brew install ffmpeg', { stdio: 'inherit' });
  } else if (platform === 'linux') {
    // Linux installation
    console.log('Installing FFmpeg on Linux...');
    // Try to detect the package manager
    let packageManager = '';
    try {
      execSync('apt --version', { stdio: 'ignore' });
      packageManager = 'apt';
    } catch (error) {
      try {
        execSync('dnf --version', { stdio: 'ignore' });
        packageManager = 'dnf';
      } catch (error) {
        try {
          execSync('yum --version', { stdio: 'ignore' });
          packageManager = 'yum';
        } catch (error) {
          try {
            execSync('pacman --version', { stdio: 'ignore' });
            packageManager = 'pacman';
          } catch (error) {
            console.error('Could not detect package manager. Please install FFmpeg manually.');
            process.exit(1);
          }
        }
      }
    }

    // Install FFmpeg using the detected package manager
    switch (packageManager) {
      case 'apt':
        execSync('sudo apt update && sudo apt install -y ffmpeg', { stdio: 'inherit' });
        break;
      case 'dnf':
        execSync('sudo dnf install -y ffmpeg', { stdio: 'inherit' });
        break;
      case 'yum':
        execSync('sudo yum install -y ffmpeg', { stdio: 'inherit' });
        break;
      case 'pacman':
        execSync('sudo pacman -S --noconfirm ffmpeg', { stdio: 'inherit' });
        break;
    }
  } else {
    console.error(`Unsupported platform: ${platform}. Please install FFmpeg manually.`);
    process.exit(1);
  }

  console.log('FFmpeg installed successfully!');
  
  // Verify installation
  const ffmpegVersion = execSync('ffmpeg -version').toString();
  console.log('Installed FFmpeg version:');
  console.log(ffmpegVersion.split('\n')[0]);
  
} catch (error) {
  console.error('Error installing FFmpeg:', error.message);
  process.exit(1);
}
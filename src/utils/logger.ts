import { info as tauriInfo, warn as tauriWarn, error as tauriError, debug as tauriDebug } from '@tauri-apps/api/log';

/**
 * Drop-in replacement for console logging using Tauri's log API.
 * All logs will be persisted to the app's log file in production.
 */

export const log = {
  info: (...args: unknown[]) => {
    tauriInfo(args.map(String).join(' '));
  },
  warn: (...args: unknown[]) => {
    tauriWarn(args.map(String).join(' '));
  },
  error: (...args: unknown[]) => {
    tauriError(args.map(String).join(' '));
  },
  debug: (...args: unknown[]) => {
    tauriDebug(args.map(String).join(' '));
  }
};

// Optionally, override global console methods (uncomment to enable globally):
// console.log = log.info;
// console.warn = log.warn;
// console.error = log.error;
// console.debug = log.debug;
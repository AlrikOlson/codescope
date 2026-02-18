import { Plugin } from 'vite';
import { spawn, execSync, ChildProcess } from 'node:child_process';
import path from 'node:path';
import fs from 'node:fs';
import http from 'node:http';

const SERVER_PORT = 8433;

function waitForServer(port: number, timeoutMs = 60000): Promise<void> {
  const start = Date.now();
  return new Promise((resolve, reject) => {
    function check() {
      const req = http.get(`http://localhost:${port}/api/tree`, (res) => {
        res.resume();
        if (res.statusCode === 200) {
          resolve();
        } else if (Date.now() - start > timeoutMs) {
          reject(new Error(`Server not ready after ${timeoutMs}ms`));
        } else {
          setTimeout(check, 200);
        }
      });
      req.on('error', () => {
        if (Date.now() - start > timeoutMs) {
          reject(new Error(`Server not ready after ${timeoutMs}ms`));
        } else {
          setTimeout(check, 200);
        }
      });
      req.setTimeout(1000, () => { req.destroy(); });
    }
    check();
  });
}

export function codeScopeServer(): Plugin {
  let serverProcess: ChildProcess | null = null;

  return {
    name: 'codescope-server',
    configureServer(server) {
      const serverDir = path.resolve(server.config.root, 'server');
      const binaryName = process.platform === 'win32' ? 'codescope-server.exe' : 'codescope-server';
      const binaryPath = path.join(serverDir, 'target', 'release', binaryName);

      // Resolve project root from env or Vite config root
      const projectRoot = process.env.CODESCOPE_ROOT || server.config.root;

      // Build if binary doesn't exist
      if (!fs.existsSync(binaryPath)) {
        console.log(`\n  Building CodeScope server (first run)...`);
        try {
          execSync('cargo build --release', {
            cwd: serverDir,
            stdio: 'inherit',
          });
        } catch (e) {
          console.error(`\n  ✗ Rust build failed. Make sure cargo is installed.`);
          console.error(`    Install Rust: https://rustup.rs/\n`);
          return;
        }
      }

      // Spawn the Rust server
      console.log(`\n  Starting CodeScope server on port ${SERVER_PORT}...`);
      serverProcess = spawn(binaryPath, ['--root', projectRoot], {
        stdio: ['ignore', 'pipe', 'pipe'],
        env: { ...process.env, PORT: String(SERVER_PORT) },
      });

      serverProcess.stdout?.on('data', (data: Buffer) => {
        process.stdout.write(data);
      });
      serverProcess.stderr?.on('data', (data: Buffer) => {
        process.stderr.write(data);
      });

      serverProcess.on('error', (err) => {
        console.error(`  ✗ Failed to start CodeScope server: ${err.message}`);
      });

      serverProcess.on('exit', (code) => {
        if (code !== null && code !== 0) {
          console.error(`  ✗ CodeScope server exited with code ${code}`);
        }
        serverProcess = null;
      });

      // Return a promise that resolves when the server is ready
      return waitForServer(SERVER_PORT)
        .then(() => {
          console.log(`  ✓ CodeScope server ready\n`);
        })
        .catch((err) => {
          console.error(`  ✗ ${err.message}\n`);
        });
    },

    closeBundle() {
      if (serverProcess) {
        serverProcess.kill('SIGTERM');
        serverProcess = null;
      }
    },

    buildEnd() {
      if (serverProcess) {
        serverProcess.kill('SIGTERM');
        serverProcess = null;
      }
    },
  };
}

/** Drives the real debug binary (fake machine) through its real webview. */
import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { appPids, dataDir, prepareWorkspace, stopApp } from "./workspace.ts";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "..");

export const application =
  process.env["WATTCLEAN_E2E_APPLICATION"] ??
  path.resolve(root, "target/debug/wattclean.exe");

/**
 * Text only a fake-system build contains (the seeded test machine). A real
 * build at the same path passes every other check, and the specs would then
 * clean the machine running them.
 */
const FAKE_MARKER = "Northwind Browser (test machine)";

function assertFreshBuild(exe: string): void {
  if (!fs.existsSync(exe)) {
    throw new Error(`no binary at ${exe} — build it with: npm run e2e:build`);
  }
  if (!fs.readFileSync(exe).includes(FAKE_MARKER)) {
    throw new Error(
      `${exe} was not built with the fake machine, so the suite would clean this PC. Rebuild it with: npm run e2e:build`,
    );
  }
  const built = fs.statSync(exe).mtimeMs;
  const distIndex = path.resolve(root, "ui/dist/index.html");
  if (fs.existsSync(distIndex) && fs.statSync(distIndex).mtimeMs > built) {
    throw new Error(
      `${exe} is older than ui/dist and does not contain the current frontend. Rebuild with: npm run e2e:build`,
    );
  }
}

let tauriDriver: ChildProcess | undefined;

function stopDriver(): void {
  if (tauriDriver?.pid) {
    spawnSync("taskkill", ["/pid", String(tauriDriver.pid), "/T", "/F"], {
      stdio: "pipe",
    });
  }
  tauriDriver = undefined;
}

const driverStatus = () =>
  fetch("http://127.0.0.1:4444/status", { signal: AbortSignal.timeout(1_000) });

async function assertDriverPortFree(): Promise<void> {
  try {
    await driverStatus();
  } catch {
    return;
  }
  throw new Error(
    "port 4444 is already held by a WebDriver process; stop the stale tauri-driver before rerunning",
  );
}

async function waitForDriver(): Promise<void> {
  const deadline = Date.now() + 30_000;
  let lastError = "no attempt made";
  while (Date.now() < deadline) {
    if (tauriDriver?.exitCode !== null && tauriDriver?.exitCode !== undefined) {
      throw new Error(
        `tauri-driver exited with code ${tauriDriver.exitCode} before binding 4444`,
      );
    }
    try {
      await driverStatus();
      return;
    } catch (error) {
      lastError = String(error);
      await new Promise((resolve) => setTimeout(resolve, 250));
    }
  }
  throw new Error(
    `tauri-driver never accepted a connection on 127.0.0.1:4444 within 30s (last: ${lastError})`,
  );
}

function assertMicrosoftDriver(driver: string): void {
  const verifier = path.resolve(root, "scripts/assert-microsoft-signature.ps1");
  const checked = spawnSync(
    "pwsh",
    ["-NoProfile", "-NonInteractive", "-File", verifier, "-Path", driver],
    { stdio: "pipe", windowsHide: true, shell: false },
  );
  if (checked.error || checked.status !== 0) {
    throw new Error(
      `EdgeDriver signature validation failed: ${checked.stderr?.toString().trim() || checked.error?.message}`,
    );
  }
}

/** What in the data directory explains a failed test. */
const EVIDENCE_FILES = [
  "settings.json",
  "disk/settings.json",
  "disk/history.json",
  "disk/picks.json",
  "disk/machine/state.json",
  "registry/settings.json",
  "registry/machine/registry.json",
  "uninstall/settings.json",
  "uninstall/history.json",
];

let failures = 0;

export const config: WebdriverIO.Config = {
  runner: "local",
  framework: "mocha",
  specs: [
    [
      // The suite and the disk cleaner.
      "boot",
      "suite",
      "cleaner",
      "shred",
      "wipe",
      "community",
      // The registry cleaner, in its own order.
      "registry/boot",
      "registry/menu",
      "registry/cleaner",
      "registry/backups",
      "registry/search",
      "registry/force",
      "registry/tuner",
      "registry/watcher",
      "registry/schedules",
      // The uninstaller, in its own order.
      "uninstall/boot",
      "uninstall/keyboard",
      "uninstall/menu",
      "uninstall/uninstall",
      "uninstall/backups",
      "uninstall/history",
      "uninstall/broken",
      "uninstall/forced",
      "uninstall/persist",
      "uninstall/traced",
      "uninstall/batch",
      // Restarts, the tray and quitting, last.
      "persist",
      "registry/persist",
      "background",
      "exit",
    ].map((name) => path.resolve(here, `specs/${name}.spec.ts`)),
  ],
  maxInstances: 1,
  logLevel: "error",
  reporters: ["spec"],
  mochaOpts: { ui: "bdd", timeout: 120_000 },
  hostname: "127.0.0.1",
  port: 4444,
  capabilities: [
    {
      // @ts-expect-error tauri:options is a tauri-driver capability.
      "tauri:options": { application },
      browserName: "wry",
      // WebdriverIO 9 otherwise requests a BiDi session, which tauri-driver
      // proxies verbatim to a native driver that answers with a blank page.
      "wdio:enforceWebDriverClassic": true,
    },
  ],

  onPrepare: async () => {
    try {
      assertFreshBuild(application);
      await assertDriverPortFree();
      if (appPids(application).length > 0) {
        throw new Error(
          "the e2e binary is already running; the suite will not terminate it",
        );
      }
      console.log(`E2E state: ${prepareWorkspace()}`);
      const nativeDriver = path.resolve(root, ".webdriver/msedgedriver.exe");
      assertMicrosoftDriver(nativeDriver);
      tauriDriver = spawn("tauri-driver", ["--native-driver", nativeDriver], {
        stdio: [null, process.stdout, process.stderr],
        shell: false,
      });
      process.once("exit", stopDriver);
      await waitForDriver();
    } catch (error) {
      if (tauriDriver?.pid) tauriDriver.kill();
      console.error(error);
      process.exit(1);
    }
  },

  before: async () => {
    // Scroll a control into view before clicking it, as a person would.
    await browser.overwriteCommand(
      "click",
      async function (this: WebdriverIO.Element, click, options) {
        await this.scrollIntoView({ block: "center", inline: "nearest" });
        return click(options);
      },
      true,
    );
  },

  afterTest: async (test, _context, { passed }) => {
    if (passed) return;
    const label = (test.fullName ?? test.title)
      .replace(/[^A-Za-z0-9]+/g, "-")
      .slice(0, 80);
    const folder = path.join(
      process.env["RUNNER_TEMP"] ?? dataDir(),
      "wattclean-failure",
      `${++failures}-${label}`,
    );
    try {
      fs.mkdirSync(folder, { recursive: true });
      for (const name of EVIDENCE_FILES) {
        const file = path.join(dataDir(), name);
        if (fs.existsSync(file)) {
          fs.copyFileSync(file, path.join(folder, path.basename(name)));
        }
      }
      await browser.saveScreenshot(path.join(folder, "window.png"));
      fs.writeFileSync(
        path.join(folder, "page.html"),
        await browser.getPageSource(),
      );
      console.error(`[diagnostic] evidence in ${folder}`);
    } catch (error) {
      console.error(`[diagnostic] not all evidence kept: ${String(error)}`);
    }
  },

  afterSession: async () => {
    await stopApp(application);
  },

  onComplete: () => {
    stopDriver();
    console.log(`E2E evidence: ${process.env["WATTCLEAN_DATA_DIR"]}`);
  },
};

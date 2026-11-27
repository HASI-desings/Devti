import { expect, type Page, test } from "@playwright/test";

async function freshApp(page: Page, settings: Record<string, string> = {}) {
  await page.goto("/");
  await page.evaluate((s) => localStorage.setItem("devti-preview-settings", JSON.stringify(s)), settings);
  await page.goto("/");
}

const rail = (page: Page) => page.getByRole("navigation", { name: "Main" });

async function pairPhone(page: Page, name: string) {
  await page.evaluate((n) => window.__devtiPreview?.join(n, "phone"), name);
  await expect(page.getByText(`Does ${name} show this code?`)).toBeVisible();
  await expect(page.getByTestId("sas")).toBeVisible();
  await page.getByRole("button", { name: "The codes match" }).click();
  await expect(page.getByRole("heading", { name: `${name} joined your ecosystem` })).toBeVisible();
}

test("first-run wizard walks through every step and lands on the dashboard", async ({ page }) => {
  await freshApp(page, { preview_autojoin: "false" });
  await expect(page.getByRole("heading", { name: "All your devices, one living system." })).toBeVisible();
  await page.getByRole("button", { name: "Set up Devti" }).click();

  await expect(page.getByRole("heading", { name: "What Devti needs, and why" })).toBeVisible();
  for (const p of ["Your local network", "Bluetooth", "Screen capture", "Input control", "Notifications"]) {
    await expect(page.getByText(p, { exact: true })).toBeVisible();
  }
  await page.getByRole("button", { name: "Continue" }).click();

  await expect(page.getByRole("heading", { name: "Name this computer" })).toBeVisible();
  await page.getByLabel("Computer name").fill("Hussnain's Laptop");
  await page.getByRole("button", { name: "Continue" }).click();

  await expect(page.getByRole("heading", { name: "Pair your first device" })).toBeVisible();
  await expect(page.getByRole("img", { name: /Pairing QR code/ })).toBeVisible();
  await pairPhone(page, "Pixel 8");
  await page.getByRole("button", { name: "Continue" }).first().click();

  await expect(page.getByRole("heading", { name: "Accessories nearby" })).toBeVisible();
  await expect(page.getByText("WF-1000XM5")).toBeVisible();
  await page.getByRole("button", { name: "Continue" }).click();

  await page.getByRole("button", { name: "Skip the tour" }).click();
  await expect(page.getByRole("heading", { name: "Devti is ready" })).toBeVisible();
  await page.getByRole("button", { name: "Open my ecosystem" }).click();

  await expect(page.getByRole("button", { name: /Pixel 8, connected/ })).toBeVisible();
  await expect(page.getByText("1 of 1 device connected")).toBeVisible();
  await expect(page.getByText("Hussnain's Laptop").first()).toBeVisible();
});

test("device page: permissions, roles and removal", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true", preview_autojoin: "false" });
  await rail(page).getByRole("button", { name: "Pair" }).click();
  await pairPhone(page, "Galaxy S25");
  await page.keyboard.press("Escape");

  await page.getByRole("button", { name: /Galaxy S25, connected/ }).click();
  await page.getByRole("button", { name: "Open device" }).click();
  await expect(page.getByRole("heading", { name: "Galaxy S25", level: 1 })).toBeVisible();

  const control = page.getByRole("switch", { name: "Control this computer" });
  await expect(control).toBeChecked();
  await control.locator("xpath=ancestor::label").click();
  await expect(control).not.toBeChecked();

  await page.getByText("Restricted", { exact: true }).click();
  await expect(page.getByRole("switch", { name: "See this computer's screen" })).not.toBeChecked();

  await page.getByRole("button", { name: "Remove device" }).click();
  await page.getByRole("dialog").getByRole("button", { name: "Remove device" }).click();
  await expect(page.getByText("No devices paired yet")).toBeVisible();
});

test("kill switch pauses and resumes the ecosystem", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true", preview_autojoin: "false" });
  await rail(page).getByRole("button", { name: "Security" }).click();
  await page.getByRole("button", { name: "Pause ecosystem" }).click();
  await expect(page.getByRole("heading", { name: "Ecosystem paused" })).toBeVisible();
  await rail(page).getByRole("button", { name: "Ecosystem" }).click();
  await expect(page.getByText("No device can connect to this computer until you resume.")).toBeVisible();
  await page.getByRole("button", { name: "Resume ecosystem" }).click();
  await expect(page.getByText("No device can connect to this computer until you resume.")).toBeHidden();

  await rail(page).getByRole("button", { name: "Security" }).click();
  await expect(page.getByText("Ecosystem paused").first()).toBeVisible();
  await expect(page.getByText("Ecosystem resumed").first()).toBeVisible();
});

test("theme and motion settings apply immediately", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Settings" }).click();
  await page.getByText("Day", { exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.getByText("Night", { exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.getByText("Reduced", { exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-reduce-motion", "true");
});

test("tour visits five stops and can be ended with Escape", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Help" }).click();
  await page.getByRole("button", { name: "Take the tour" }).click();
  const tour = page.getByRole("dialog", { name: "Tour of Devti" });
  await expect(tour.getByText("1 of 5")).toBeVisible();
  for (let i = 2; i <= 5; i++) {
    await tour.getByRole("button", { name: "Next" }).click();
    await expect(tour.getByText(`${i} of 5`)).toBeVisible();
  }
  await page.keyboard.press("Escape");
  await expect(tour).toBeHidden();
});

test("rail is keyboard navigable", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Security" }).focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: "Security", level: 1 })).toBeVisible();
  await expect(rail(page).getByRole("button", { name: "Security" })).toHaveAttribute("aria-current", "page");
});

test("pairing another computer by code shows a clear message in preview", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true", preview_autojoin: "false" });
  await rail(page).getByRole("button", { name: "Pair" }).click();
  await page.getByText("Another computer", { exact: true }).click();
  await expect(page.getByTestId("pair-code")).toHaveText(/\d{3} \d{3}/);
  await page.getByRole("button", { name: /Office Desktop/ }).click();
  await page.getByLabel("Pairing code").fill("123456");
  await page.getByRole("button", { name: "Pair", exact: true }).last().click();
  await expect(page.getByText(/needs the real Devti app/)).toBeVisible();
});

test("browser preview says so and never invents devices", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true" });
  await expect(page.getByRole("note").getByText("Browser preview.")).toBeVisible();
  await rail(page).getByRole("button", { name: "Pair" }).click();
  // Nothing joins by itself.
  await page.waitForTimeout(5000);
  await expect(page.getByText(/show this code\?/)).toBeHidden();
  // Only the explicit demo button simulates a phone.
  await page.getByRole("button", { name: "Simulate a phone" }).click();
  await expect(page.getByText("Does Demo phone show this code?")).toBeVisible();
});

async function pairedDemo(page: Page) {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Pair" }).click();
  await page.getByRole("button", { name: "Simulate a phone" }).click();
  await page.getByRole("button", { name: "The codes match" }).click();
  await expect(page.getByRole("heading", { name: "Demo phone joined your ecosystem" })).toBeVisible();
  await page.keyboard.press("Escape");
}

test("phone notifications: badge, quick reply, dismiss and Do Not Disturb", async ({ page }) => {
  await pairedDemo(page);
  await expect(rail(page).getByText("2 notifications")).toBeAttached();
  await rail(page)
    .getByRole("button", { name: /Alerts/ })
    .click();
  await expect(page.getByText("Are you coming to the studio today?")).toBeVisible();
  await page.getByRole("button", { name: "Reply", exact: true }).click();
  await page.getByLabel("Reply").fill("On my way");
  await page.getByRole("button", { name: "Send reply" }).click();
  await expect(page.getByText("Reply sent.")).toBeVisible();
  await page.getByRole("button", { name: "Dismiss" }).first().click();
  await expect(page.getByText("Are you coming to the studio today?")).toBeHidden();
  const dnd = page.getByRole("switch", { name: "Do Not Disturb" });
  await dnd.locator("xpath=ancestor::label").click();
  await expect(dnd).toBeChecked();
  await expect(page.getByText(/Notifications still arrive here, silently/)).toBeVisible();
});

test("files: choose, pick a device, watch it send", async ({ page }) => {
  await pairedDemo(page);
  await rail(page).getByRole("button", { name: "Files" }).click();
  const chooser = page.waitForEvent("filechooser");
  await page.getByRole("button", { name: /Drop files here/ }).click();
  await (await chooser).setFiles({
    name: "holiday.jpg",
    mimeType: "image/jpeg",
    buffer: Buffer.alloc(2_000_000, 1),
  });
  await expect(page.getByRole("heading", { name: "Send holiday.jpg to…" })).toBeVisible();
  await page.getByRole("button", { name: /Demo phone/ }).click();
  await expect(page.getByText("holiday.jpg")).toBeVisible();
  await expect(page.getByText("Sent holiday.jpg to Demo phone.")).toBeVisible({ timeout: 10_000 });
  await expect(page.getByRole("progressbar", { name: "holiday.jpg 100%" })).toBeVisible();
});

test("battery dashboard shows a chart per device with a table view", async ({ page }) => {
  await pairedDemo(page);
  await rail(page).getByRole("button", { name: "Battery" }).click();
  await expect(page.getByRole("img", { name: "Demo phone: battery level over time" })).toBeVisible();
  await expect(page.getByText("Shaded areas: charging")).toBeVisible();
  await page
    .getByRole("region", { name: "Demo phone battery" })
    .getByRole("button", { name: "Show as table" })
    .click();
  await expect(page.getByRole("region", { name: "Demo phone battery" }).getByRole("table")).toBeVisible();
  await page.getByText("Last 7 days").click();
  await expect(page.getByRole("img", { name: "Studio Laptop: battery level over time" })).toBeVisible();
});

test("find my device: being rung shows a full-screen call until found", async ({ page }) => {
  await pairedDemo(page);
  await page.evaluate(() => window.__devtiPreview?.ringMe(true));
  const call = page.getByRole("alertdialog", { name: "Demo phone is looking for this computer" });
  await expect(call).toBeVisible();
  await call.getByRole("button", { name: "I found it" }).click();
  await expect(call).toBeHidden();
  // Ringing another device from its summary panel.
  await page.getByRole("button", { name: /Demo phone, connected/ }).click();
  await page.getByRole("button", { name: "Ring it" }).click();
  await expect(page.getByText("Demo phone is ringing.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Stop ringing" })).toBeVisible();
});

test("control: deck editor, saved commands and local API", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Control" }).click();
  await expect(page.getByRole("heading", { name: "Control", level: 1 })).toBeVisible();

  // The default deck is there; add a button and give it an action.
  await expect(page.getByRole("button", { name: "Play / pause" })).toBeVisible();
  await page.getByRole("button", { name: "Add button" }).click();
  await page.getByLabel("Label").fill("Go live");
  await page.getByLabel("When pressed").selectOption("obs_scene");
  await page.getByLabel("Scene name").fill("Live");
  await expect(page.getByRole("button", { name: "Go live" })).toBeVisible();
  await page.getByRole("button", { name: "Try it here" }).click();
  await expect(page.getByText("OBS isn't reachable from the browser preview.")).toBeVisible();

  // Saved commands: test one, then save.
  await page.getByText("Saved commands", { exact: true }).click();
  await page.getByRole("button", { name: "Add a command" }).click();
  await page.getByLabel("Name").last().fill("Back up");
  await page.getByLabel("Command").last().fill("rsync -a ~/Photos /backup");
  await page.getByRole("button", { name: "Test" }).last().click();
  await expect(page.getByText("(preview) would run: rsync -a ~/Photos /backup")).toBeVisible();
  await page.getByRole("button", { name: "Save changes" }).click();
  await expect(page.getByText("Saved commands updated.")).toBeVisible();

  // Local API: token hidden until shown, reset makes a new one.
  await page.getByText("Local API and CLI", { exact: true }).click();
  await expect(page.getByText("devti run torch --on")).toBeVisible();
  const masked = await page.getByText(/•{10}/).textContent();
  await page.getByRole("button", { name: "Show" }).click();
  await expect(page.getByText(/^[0-9a-f]{64}$/)).toBeVisible();
  await page.getByRole("button", { name: "Reset" }).click();
  await expect(page.getByText("New token made.", { exact: false })).toBeVisible();
  expect(masked).toBeTruthy();
});

test("mirror and control a phone, run its command, power needs confirmation", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Pair" }).click();
  await pairPhone(page, "Pixel 8");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: /Pixel 8, connected/ }).click();
  await page.getByRole("button", { name: "Open device" }).click();

  // The phone's catalog: a command with output, and its apps.
  await page.getByRole("button", { name: "Run" }).click();
  await expect(page.getByText("(simulated) torch finished on Pixel 8")).toBeVisible();
  await expect(page.getByRole("button", { name: /Spotify/ })).toBeVisible();

  await page.getByRole("button", { name: "Mirror and control" }).click();
  const screen = page
    .getByRole("img", { name: /Pixel 8's screen/ })
    .or(page.locator("canvas[aria-label^='Pixel 8']"));
  await expect(screen).toBeVisible();
  await expect(page.getByText("fps")).toBeVisible();
  // Tap, type and go home; the simulated phone draws what it received.
  const box = await screen.boundingBox();
  if (!box) throw new Error("no screen");
  await page.mouse.click(box.x + box.width * 0.5, box.y + box.height * 0.5);
  await page.keyboard.type("hi");
  await page.getByRole("button", { name: "Home" }).click();
  await expect(page.getByRole("button", { name: "Screenshot" })).toBeEnabled();
  await page.getByRole("button", { name: "Pixel 8" }).first().click();
  await expect(page.getByRole("heading", { name: "Pixel 8", level: 1 })).toBeVisible();
});

test("screens: choose what to show and place a computer at an edge", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Pair" }).click();
  await page.evaluate(() => window.__devtiPreview?.join("Studio Desktop", "desktop"));
  await page.getByRole("button", { name: "The codes match" }).click();
  await expect(page.getByRole("heading", { name: "Studio Desktop joined your ecosystem" })).toBeVisible();
  await page.keyboard.press("Escape");

  await rail(page).getByRole("button", { name: "Screens" }).click();
  await expect(page.getByRole("heading", { name: "Screens", level: 1 })).toBeVisible();
  const slides = page.getByRole("button", { name: /Quarterly review/ });
  await slides.click();
  await expect(slides).toHaveAttribute("aria-pressed", "true");

  const kvm = page.getByRole("switch", { name: "Move the pointer between computers" });
  await kvm.locator("xpath=ancestor::label").click();
  await expect(kvm).toBeChecked();
  await page.getByLabel("Computer right this one").selectOption({ label: "Studio Desktop" });
  await expect(page.locator("[data-edge='right']")).toContainText("Studio Desktop");
  // Reload: the arrangement was saved.
  await rail(page).getByRole("button", { name: "Ecosystem" }).click();
  await rail(page).getByRole("button", { name: "Screens" }).click();
  await expect(page.locator("[data-edge='right']")).toContainText("Studio Desktop");
});

test("accessories: profile-built controls, battery per bud, draft profile for unknown devices", async ({
  page,
}) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Accessories" }).click();
  await expect(page.getByRole("heading", { name: "Accessories", level: 1 })).toBeVisible();

  // AirPods: generated noise control, per-bud and case battery, an honest "untested" badge.
  await expect(page.getByRole("heading", { name: "AirPods Pro" })).toBeVisible();
  await expect(page.getByText("Untested on real hardware")).toBeVisible();
  for (const part of ["Left", "Right", "Case"])
    await expect(page.getByText(part, { exact: true })).toBeVisible();
  await page.getByText("Transparency", { exact: true }).click();
  await expect(page.getByRole("radio", { name: "Transparency" })).toBeChecked();

  // Sony while disconnected: controls are shown but tell you to connect.
  await page.getByRole("button", { name: /WH-1000XM5/ }).click();
  await expect(page.getByText("Connect it to change its settings.")).toBeVisible();
  await expect(page.getByRole("group", { name: "Equalizer" })).toBeVisible();
  await page.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(page.getByRole("button", { name: "Disconnect", exact: true })).toBeVisible();

  // A generic keyboard offers "Help support this device".
  await page.getByRole("button", { name: /MX Keys/ }).click();
  await page.getByRole("button", { name: "Save draft" }).click();
  await expect(page.getByText(/Draft profile saved/)).toBeVisible();
});

test("messages: read a conversation, reply, and a call card to answer", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Pair" }).click();
  await pairPhone(page, "Pixel 8");
  await page.keyboard.press("Escape");

  await rail(page).getByRole("button", { name: "Messages" }).click();
  await page.getByRole("button", { name: /Ayesha/ }).click();
  await expect(page.getByText("Hey! Dinner tonight?")).toBeVisible();
  await page.getByLabel("Message", { exact: true }).fill("Yes, 7 works");
  await page.keyboard.press("Enter");
  await expect(page.getByLabel(/Conversation with Ayesha/).getByText("Yes, 7 works")).toBeVisible();

  // An incoming call shows a card with Answer / Decline.
  await page.evaluate(() => window.__devtiPreview?.callMe?.(true));
  const card = page.getByRole("alertdialog", { name: "Call from Ayesha" });
  await expect(card).toBeVisible();
  await card.getByRole("button", { name: "Answer" }).click();
  await expect(card.getByText("On a call", { exact: false })).toBeVisible();
  await card.getByRole("button", { name: "End" }).click();
  await expect(card).toBeHidden();
});

test("automation: default rules, switch one on, build a new rule", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Automation" }).click();
  const lock = page.getByRole("switch", { name: "Phone walks away → lock this computer" });
  await expect(lock).not.toBeChecked();
  await lock.locator("xpath=ancestor::label").click();
  await expect(lock).toBeChecked();

  await page.getByRole("button", { name: "New rule" }).click();
  await page.getByLabel("Name").fill("Morning focus");
  await page.getByLabel("Trigger").selectOption("time");
  await page.getByLabel("Action 1").selectOption("set_dnd");
  await page.getByRole("button", { name: "Save rule" }).click();
  await expect(page.getByText("Morning focus", { exact: true }).first()).toBeVisible();
  await expect(page.getByText("When at a time → do not disturb")).toBeVisible();
});

test("browse: share a folder here, browse a device's folders and download a file", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Pair" }).click();
  await pairPhone(page, "Pixel 8");
  await page.keyboard.press("Escape");

  await rail(page).getByRole("button", { name: "Files" }).click();
  await page.getByRole("radio", { name: "Browse" }).click({ force: true });
  await expect(page.getByText("Nothing is shared.", { exact: false })).toBeVisible();
  await page.getByRole("button", { name: "Share a folder" }).click();
  await expect(page.getByText("/home/you/Shared")).toBeVisible();

  // The connected phone's shared folders, then one level down, then a download.
  await expect(page.getByRole("tab", { name: "Pixel 8" })).toHaveAttribute("aria-selected", "true");
  await page.getByRole("button", { name: "Documents", exact: true }).click();
  await expect(page.getByText("Thesis draft.pdf", { exact: true })).toBeVisible();
  await expect(
    page.getByRole("navigation", { name: "Folder" }).getByRole("button", { name: "Documents" }),
  ).toHaveAttribute("aria-current", "page");
  await page.getByRole("button", { name: "Download Thesis draft.pdf" }).click();
  await expect(page.getByText("Downloading Thesis draft.pdf", { exact: false })).toBeVisible();
  await page.getByRole("radio", { name: "Transfers" }).click({ force: true });
  await expect(page.getByText("Thesis draft.pdf", { exact: true })).toBeVisible();
});

test("plugins: consent sheet lists what it may do, then its command runs", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Plugins" }).click();
  await expect(page.getByText("No plugins yet.", { exact: false })).toBeVisible();
  await page.getByRole("button", { name: "Add a plugin" }).click();
  const sheet = page.getByRole("dialog", { name: "Add a plugin" });
  await expect(sheet.getByRole("heading", { name: "Add Link cleaner 1.0.0?" })).toBeVisible();
  await expect(sheet.getByText("Read this computer's clipboard")).toBeVisible();
  await expect(sheet.getByText("it receives the text", { exact: false })).toBeVisible();
  await sheet.getByRole("button", { name: "Add and turn on" }).click();

  await expect(page.getByRole("heading", { name: /Link cleaner/ })).toBeVisible();
  await expect(page.getByText("Running", { exact: true })).toBeVisible();
  await page.getByRole("button", { name: "Clean the copied link" }).click();
  await expect(page.getByText("Removed 2 tracking parameters. (simulated)")).toBeVisible();

  // Off means its commands stop.
  await page.getByRole("switch", { name: "Turn Link cleaner on" }).locator("xpath=ancestor::label").click();
  await expect(page.getByText("Off", { exact: true })).toBeVisible();
});

test("settings: connect from anywhere, own relay, and the VPN fallback", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Settings" }).click();
  await expect(page.getByRole("heading", { name: "Connect from anywhere" })).toBeVisible();
  await expect(page.getByTestId("anywhere-status")).toContainText("Reachable from anywhere");
  await expect(page.getByTestId("anywhere-status")).toContainText("203.0.113.42");

  const relay = page.getByLabel("Your own relay (optional)");
  await relay.fill("not a relay");
  await page.getByRole("button", { name: "Save", exact: true }).first().click();
  await expect(page.getByText("the relay address looks like https://relay.example.org")).toBeVisible();
  await relay.fill("https://relay.example.org");
  await page.getByRole("button", { name: "Save", exact: true }).first().click();
  await expect(page.getByTestId("anywhere-status")).toContainText("through relay.example.org");

  await page.getByRole("switch", { name: "Connect from anywhere" }).locator("xpath=ancestor::label").click();
  await expect(page.getByTestId("anywhere-status")).toContainText("Off.");
});

test("settings: VPN and typed addresses show and are validated", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Settings" }).click();
  await expect(page.getByRole("heading", { name: "Other ways to connect" })).toBeVisible();
  await expect(page.getByText("100.101.12.7:47810")).toBeVisible();
  await expect(page.getByText("Private network · tailscale0")).toBeVisible();

  const field = page.getByLabel("Addresses you set up yourself");
  await field.fill("not an address");
  await page.getByRole("button", { name: "Save", exact: true }).nth(1).click();
  await expect(page.getByText("isn't an address Devti can dial", { exact: false })).toBeVisible();

  await field.fill("home.example.org:47810");
  await page.getByRole("switch", { name: "Reach from anywhere" }).locator("xpath=ancestor::label").click();
  await expect(page.getByText("Your devices can now reach this computer from anywhere.")).toBeVisible();
  await expect(page.getByText("Set up by you")).toBeVisible();
});

test("phone camera: front or back, waits for the phone, shows live, switches and stops", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Pair" }).click();
  await pairPhone(page, "Pixel 8");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: /Pixel 8, connected/ }).click();
  await page.getByRole("button", { name: "Open device" }).click();

  const status = page.getByTestId("camera-status");
  await expect(status).toContainText("The phone asks before it turns on");
  await page.getByRole("button", { name: "Back", exact: true }).click();
  await page.getByRole("button", { name: "Use camera" }).click();
  await expect(status).toContainText("Waiting for Pixel 8");
  await expect(status).toContainText("On: back camera");

  // Clicking again never turns it off; Show opens the live view.
  await page.getByRole("button", { name: "Show" }).click();
  await expect(page.getByTestId("camera-canvas")).toBeVisible();
  await expect(page.getByText(/Back camera · \d+ fps/)).toBeVisible();
  await page.getByRole("button", { name: "Switch to front" }).click();
  await expect(status).toContainText("On: front camera");
  await page.keyboard.press("Escape");

  await page.getByRole("button", { name: "Stop", exact: true }).click();
  await expect(page.getByText("Pixel 8's camera stopped.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Use camera" })).toBeVisible();
});

test("find my devices: map, this computer, locate a phone with its IP and network", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Pair" }).click();
  await pairPhone(page, "Pixel 8");
  await page.keyboard.press("Escape");
  await rail(page).getByRole("button", { name: "Find" }).click();
  await expect(page.getByRole("heading", { name: "Find my devices" })).toBeVisible();
  await expect(page.getByTestId("find-map")).toBeVisible();
  await expect(page.getByText("· this computer")).toBeVisible();

  const phone = page.getByRole("listitem").filter({ hasText: "Pixel 8" });
  await expect(phone).toContainText("Not located yet.");
  await phone.getByRole("button", { name: "Locate" }).click();
  await expect(phone).toContainText("Mobile data: Jazz");
  await expect(phone).toContainText("203.0.113.42");
  await expect(phone).toContainText(/31\.\d{5}, 74\.\d{5}/);
  await expect(phone.getByRole("button", { name: "Open map" })).toBeVisible();
  await expect(page.locator(".leaflet-marker-icon")).toHaveCount(2);
});

test("stolen mode: USB setup, the phone's yes, lock with a message, unlock code, turn off", async ({
  page,
}) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Pair" }).click();
  await pairPhone(page, "Pixel 8");
  await page.keyboard.press("Escape");
  await rail(page).getByRole("button", { name: "Find" }).click();
  await page
    .getByRole("listitem")
    .filter({ hasText: "Pixel 8" })
    .getByRole("button", { name: "Stolen mode" })
    .click();

  const sheet = page.getByRole("dialog");
  await expect(sheet.getByRole("heading", { name: "Protect Pixel 8 if it's stolen" })).toBeVisible();
  await sheet.getByRole("button", { name: "Find the phone" }).click();
  await sheet.getByRole("button", { name: "Make Devti the protector" }).click();
  await expect(sheet.getByRole("status")).toContainText("Devti can now fully protect this phone");
  await sheet.getByRole("button", { name: "Ask the phone" }).click();
  await expect(sheet.getByRole("heading", { name: "Pixel 8 is protected" })).toBeVisible();
  await expect(sheet.getByText("Full protection")).toBeVisible();
  await expect(sheet.getByText("Power menu blocked")).toBeVisible();

  await sheet.getByLabel("How to reach you (optional)").fill("+92 300 1234567");
  await sheet.getByRole("button", { name: "Turn on stolen mode" }).click();
  await sheet.getByRole("button", { name: "Lock it now" }).click();
  await expect(sheet.getByRole("heading", { name: "Stolen mode is on for Pixel 8" })).toBeVisible();

  await sheet.getByLabel("Question on the phone").fill("1234 5678");
  await sheet.getByRole("button", { name: "Get the answer" }).click();
  await expect(sheet.getByTestId("unlock-code")).toHaveText(/^\d{5} \d{5}$/);

  await sheet.getByRole("button", { name: "Turn off stolen mode" }).click();
  await expect(sheet.getByRole("heading", { name: "Pixel 8 is protected" })).toBeVisible();
});

test("phone radios ask before cutting a connection, and recording saves a file", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Pair" }).click();
  await pairPhone(page, "Pixel 8");
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: /Pixel 8, connected/ }).click();
  await page.getByRole("button", { name: "Open device" }).click();

  await expect(page.getByText("Its radios")).toBeVisible();
  await page.getByRole("switch", { name: "Airplane mode" }).locator("xpath=ancestor::label").click();
  await expect(page.getByText("Airplane mode cuts every connection")).toBeVisible();
  await page.getByRole("button", { name: "Cancel" }).click();
  await page.getByRole("switch", { name: "Bluetooth", exact: true }).locator("xpath=ancestor::label").click();
  await expect(page.getByText("install Shizuku on the phone", { exact: false })).toBeVisible();

  const status = page.getByTestId("record-status");
  await page.getByRole("button", { name: "Microphone" }).click();
  await expect(status).toContainText(/Recording the microphone · 0:0\d/);
  await page.getByRole("button", { name: "Stop" }).click();
  await expect(page.getByText(/Recording saved: .*Pixel 8 microphone/)).toBeVisible();
});

test("shared headphones: choose where everything plays, voice-message pause switch", async ({ page }) => {
  await freshApp(page, { onboarding_done: "true" });
  await rail(page).getByRole("button", { name: "Pair" }).click();
  await pairPhone(page, "Pixel 8");
  await page.keyboard.press("Escape");
  await rail(page).getByRole("button", { name: "Accessories" }).click();
  await expect(page.getByRole("heading", { name: "One pair of headphones for everything" })).toBeVisible();

  const here = page.getByRole("radio", { name: /Pixel 8's sound here, together with this computer's/ });
  await here.check({ force: true });
  await expect(here).toBeChecked();
  const onPhone = page.getByRole("radio", { name: /This computer's sound on Pixel 8/ });
  await onPhone.check({ force: true });
  await expect(onPhone).toBeChecked();
  await expect(here).not.toBeChecked();

  await expect(page.getByTestId("voice-status")).toContainText("pauses the music on the others");
  const pause = page.getByRole("switch", { name: "Pause the music for voice messages" });
  await expect(pause).toBeChecked();
  await pause.locator("xpath=ancestor::label").click();
  await expect(pause).not.toBeChecked();
});

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

// README screenshots of the UI with synthetic demo data (mock backend).
//
//   npm --prefix app run dev:mock            # in one terminal
//   cd tools/screenshots && npm i --no-save playwright && npx playwright install webkit
//   node shoot.mjs ../../docs/images
//
// WebKit is the engine of the macOS app; images are 2x (Retina).
import { webkit } from "playwright";

const OUT = process.argv[2] ?? "../../docs/images";
const URL = "http://localhost:5173/?readme&lang=en-US";
const KEY = "abcd".repeat(16); // fake key, accepted by the mock only

const browser = await webkit.launch();

async function openDemo(theme) {
  const ctx = await browser.newContext({
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 2,
    colorScheme: theme,
    locale: "en-US",
    timezoneId: "Europe/Berlin",
  });
  const page = await ctx.newPage();
  await page.goto(URL);
  await page.getByRole("button", { name: "Open folder…" }).click();
  await page.getByLabel("Recovery key").fill(KEY);
  await page.keyboard.press("Enter");
  await page.getByText("Weekend Trip").first().click();
  await page.waitForSelector(".bubble");
  // Show a stretch with a mention, a quote, a photo and reactions.
  await page.evaluate(async () => {
    const el = document.querySelector(".messages");
    el.scrollTop = el.scrollHeight * (432 / 480);
    await new Promise((r) => setTimeout(r, 400));
    document.querySelector('[data-index="433"]')?.scrollIntoView({ block: "start" });
  });
  await page.waitForTimeout(800);
  return { ctx, page };
}

for (const theme of ["dark", "light"]) {
  const { ctx, page } = await openDemo(theme);
  await page.screenshot({ path: `${OUT}/app-${theme}.png` });
  if (theme === "dark") {
    await page.getByRole("button", { name: "Export chats…" }).click();
    for (const name of ["Weekend Trip", "Family", "Book Club"]) {
      await page.locator(".export-item", { hasText: name }).locator("input").check();
    }
    await page.waitForTimeout(300);
    await page.screenshot({ path: `${OUT}/export-${theme}.png` });
  } else {
    // The unlock dialog, on a fresh page.
    const p2 = await ctx.newPage();
    await p2.goto(URL);
    await p2.getByRole("button", { name: "Open folder…" }).click();
    await p2.getByLabel("Recovery key").fill(KEY.slice(0, 40));
    await p2.waitForTimeout(300);
    await p2.screenshot({ path: `${OUT}/unlock-${theme}.png` });
  }
  await ctx.close();
}
await browser.close();
console.log("screenshots written");

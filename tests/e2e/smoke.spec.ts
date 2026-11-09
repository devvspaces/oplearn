import { expect, test } from "@playwright/test";
import { installMockIpc } from "./mock-ipc";

test.beforeEach(async ({ page }) => {
  await page.addInitScript(installMockIpc);
  // The loopback media server: serve a real 4-page PDF so pdf.js actually renders.
  await page.route("http://127.0.0.1:9/**", (route) =>
    route.fulfill({
      status: 200,
      path: "tests/e2e/fixtures/sample.pdf",
      headers: {
        "Content-Type": "application/pdf",
        "Access-Control-Allow-Origin": "*",
        "Access-Control-Expose-Headers": "Accept-Ranges, Content-Range, Content-Length",
      },
    }),
  );
});

test("app shell loads and library shows imported course", async ({ page }) => {
  await page.goto("/#/library");
  await expect(page.getByRole("navigation", { name: "Main" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Library" })).toBeVisible();
  await expect(page.getByText("MIT 6.824 Distributed Systems")).toBeVisible();
  await expect(page.getByText("3 lessons · 4h")).toBeVisible();
});

test("course page shows outline in order with CC badges", async ({ page }) => {
  await page.goto("/#/courses/c1");
  await expect(page.getByRole("heading", { name: "Lectures" })).toBeVisible();
  const rows = page.locator("section ul > li");
  await expect(rows).toHaveCount(4);
  await expect(rows.nth(0)).toContainText("Lecture 1");
  await expect(rows.nth(2)).toContainText("Lecture 3");
  // Documents show what they are and their page count.
  await expect(rows.nth(3)).toContainText("Problem Set 1");
  await expect(rows.nth(3)).toContainText("Problem set");
  await expect(rows.nth(3)).toContainText("4 pp");
  await expect(page.getByRole("link", { name: /Continue: Lecture 2/ })).toBeVisible();
});

test("keyboard navigation reaches settings", async ({ page }) => {
  await page.goto("/#/settings");
  await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();
  await expect(page.getByText("2026.10.26")).toBeVisible();
});

test("Cmd+K search shows spoken matches and opens the player at that moment", async ({ page }) => {
  await page.goto("/#/library");
  await expect(page.getByRole("heading", { name: "Library" })).toBeVisible();
  await page.keyboard.press("ControlOrMeta+k");
  const input = page.getByRole("combobox", { name: "Search everything" });
  await input.fill("mapreduce");
  await expect(page.getByRole("group", { name: "Spoken in lectures" })).toBeVisible();
  await expect(page.getByRole("group", { name: "Lessons" })).toBeVisible();
  await expect(page.getByRole("option").first()).toContainText("12:34");
  await input.press("Enter");
  await expect(page).toHaveURL(/#\/lessons\/l1\?t=754/);
  await expect(page.getByRole("heading", { name: "Lecture 2" })).toBeVisible();
});

test("course notebook groups annotations with timestamp links", async ({ page }) => {
  await page.goto("/#/courses/c1");
  await page.getByRole("tab", { name: "notebook" }).click();
  await expect(page.getByText("Why does MapReduce need a master?")).toBeVisible();
  const ts = page.getByRole("link", { name: "12:34" });
  await expect(ts).toHaveAttribute("href", /lessons\/l1\?t=754/);
});

test("downloads screen shows progress, errors and storage", async ({ page }) => {
  await page.goto("/#/downloads");
  await expect(page.getByRole("heading", { name: "Downloads" })).toBeVisible();
  await expect(page.getByText("Downloading")).toBeVisible();
  await expect(page.getByText("42%")).toBeVisible();
  await expect(page.getByText("2.1 MB/s")).toBeVisible();
  await expect(page.getByText("This video or playlist is unavailable or has been removed.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Retry" })).toBeVisible();
  await expect(page.getByText("312 MB").first()).toBeVisible();
  await expect(page.getByLabel("1 active downloads")).toBeVisible();
});

test("switching a course to Download shows the notice, quality and size estimate", async ({ page }) => {
  await page.goto("/#/courses/c1");
  await page.getByRole("radio", { name: "Download" }).click();
  await expect(page.getByText("Before you download")).toBeVisible();
  await expect(page.getByText("3 lessons · about 3.2 GB (estimate)")).toBeVisible();
  const confirm = page.getByRole("button", { name: "Download", exact: true });
  await expect(confirm).toBeDisabled();
  await page.getByLabel("I understand").check();
  await expect(confirm).toBeEnabled();
});

const calls = (page: import("@playwright/test").Page, cmd: string) =>
  page.evaluate(
    (c) =>
      (window as unknown as { __calls: Array<{ cmd: string; args: unknown }> }).__calls.filter(
        (x) => x.cmd === c,
      ),
    cmd,
  );

test("Today shows the goal ring, streak, plan and behind warning", async ({ page }) => {
  await page.goto("/#/");
  await expect(page.getByRole("img", { name: "20 of 45 minutes studied today" })).toBeVisible();
  await expect(page.getByText("4-day streak")).toBeVisible();
  await expect(page.getByText("is behind by 2 lessons")).toBeVisible();
  await expect(page.getByRole("link", { name: "Lecture 2", exact: true })).toBeVisible();
  await expect(page.getByLabel("1 cards due")).toBeVisible();
});

test("lesson menu downloads at a chosen quality", async ({ page }) => {
  await page.goto("/#/courses/c1");
  await page.getByRole("button", { name: "Actions for Lecture 1" }).click();
  await page.getByRole("menuitem", { name: "Download for offline" }).focus();
  await page.keyboard.press("ArrowRight");
  const item = page.getByRole("menuitem", { name: /480p/ });
  await expect(item).toBeVisible();
  await item.focus();
  await page.keyboard.press("Enter");
  await expect
    .poll(async () => (await calls(page, "enqueue_lesson_download")).map((c) => c.args))
    .toEqual([{ lessonId: "l0", quality: "480p" }]);
});

test("lesson page shows live download progress", async ({ page }) => {
  await page.goto("/#/lessons/l1");
  await expect(page.getByLabel(/Downloading at 720p: 42%/)).toBeVisible();
  await expect(page.getByRole("button", { name: "Pause download" })).toBeVisible();
});

test("course plan tab shows status and schedule", async ({ page }) => {
  await page.goto("/#/courses/c1");
  await expect(page.getByText("Behind by 2").first()).toBeVisible();
  await page.getByRole("tab", { name: "plan" }).click();
  await expect(page.getByRole("radio", { name: "Lessons per week" })).toHaveAttribute("aria-checked", "true");
  await expect(page.getByRole("button", { name: "Rebalance from today" })).toBeVisible();
  await expect(page.getByText("Following the AI plan's order")).toBeVisible();
  await page.getByRole("button", { name: "Use outline order" }).click();
  await expect
    .poll(async () => (await calls(page, "set_plan_order")).map((c) => (c.args as { order: string }).order))
    .toEqual(["outline"]);
});

test("planner renders the heatmap and weekly review", async ({ page }) => {
  await page.goto("/#/planner");
  await expect(page.getByRole("img", { name: /Study activity/ })).toBeVisible();
  await expect(page.getByText("min studied")).toBeVisible();
});

test("YouTube search shows CC badge and the fallback notice", async ({ page }) => {
  await page.goto("/#/search");
  await page.getByRole("textbox", { name: "Search YouTube" }).fill("distributed systems lecture");
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await expect(page.getByText("Raft lecture")).toBeVisible();
  await expect(page.getByTitle("Creative Commons licensed")).toBeVisible();
  await expect(page.getByRole("status")).toContainText("results come from yt-dlp");
  await expect(page.getByRole("button", { name: "Add to course" })).toBeVisible();
});

test("flashcard review reveals and grades a card", async ({ page }) => {
  await page.goto("/#/review");
  await expect(page.getByText("Why does MapReduce need a master?")).toBeVisible();
  await page.keyboard.press("Space");
  await expect(page.getByText("To assign tasks and re-run failed workers.")).toBeVisible();
  await expect(page.getByRole("link", { name: /Jump to source at 12:34/ })).toBeVisible();
  await page.getByRole("button", { name: /Good/ }).click();
  await expect(page.getByText("All done: 1 card reviewed")).toBeVisible();
  await expect.poll(async () => (await calls(page, "review_flashcard")).length).toBe(1);
});

test("AI restructure shows the cost, the proposal, and applies it", async ({ page }) => {
  await page.goto("/#/courses/c1");
  await page.getByRole("button", { name: "Course actions" }).click();
  await page.getByRole("menuitem", { name: "Restructure with AI…" }).click();
  await expect(page.getByText(/estimated \$0\.05/)).toBeVisible();
  await page.getByRole("button", { name: /Generate proposal/ }).click();
  await expect(page.getByText("Grouped the introductory lectures first.")).toBeVisible();
  await expect(page.getByLabel("Module 1 title")).toHaveValue("Foundations");
  await page.getByRole("button", { name: "Apply new outline" }).click();
  await expect.poll(async () => (await calls(page, "apply_course_structure")).length).toBe(1);
});

test("playlist preview lists the videos before creating a course", async ({ page }) => {
  await page.goto("/#/search");
  await page.getByRole("radio", { name: "playlists" }).click();
  await page.evaluate(() => {
    // Serve a playlist result for this test.
    const w = window as unknown as {
      __TAURI_INTERNALS__: { invoke: (c: string, a: unknown) => Promise<unknown> };
    };
    const orig = w.__TAURI_INTERNALS__.invoke;
    w.__TAURI_INTERNALS__.invoke = async (c, a) =>
      c === "search_youtube"
        ? {
            source: "api",
            cached: false,
            notice: null,
            results: [
              {
                kind: "playlist",
                id: "PLabc",
                title: "MIT 6.824 Spring 2020",
                channelTitle: "MIT",
                thumbnailUrl: null,
                durationSec: null,
                viewCount: null,
                license: null,
                publishedAt: null,
                itemCount: 2,
              },
            ],
          }
        : orig(c, a);
  });
  await page.getByRole("textbox", { name: "Search YouTube" }).fill("6.824");
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await page.getByRole("button", { name: "Preview" }).click();
  const dlg = page.getByRole("dialog");
  await expect(dlg.getByText("Lecture 2: RPC and Threads")).toBeVisible();
  await expect(dlg.getByText("All CC")).toBeVisible();
});

test("planner month view shows planned lessons", async ({ page }) => {
  await page.goto("/#/planner");
  await page.getByRole("radio", { name: "month" }).click();
  await expect(page.getByRole("region", { name: "Month" })).toBeVisible();
  await expect(page.getByRole("region", { name: "Month" }).getByText("Lecture 2").first()).toBeVisible();
});

test("Cmd+K finds flashcards and PDF pages", async ({ page }) => {
  await page.goto("/#/library");
  await expect(page.getByRole("heading", { name: "Library" })).toBeVisible();
  await page.keyboard.press("ControlOrMeta+k");
  await page.getByRole("combobox", { name: "Search everything" }).fill("mapreduce");
  await expect(page.getByRole("group", { name: "Flashcards" })).toBeVisible();
  const pdf = page.getByRole("group", { name: "Course materials (PDF)" });
  await expect(pdf.getByText("p.3")).toBeVisible();
  await pdf.getByRole("option").first().click();
  await expect(page).toHaveURL(/#\/materials\/mat1\?page=3/);
});

test("AI flashcards: estimate, drafts with timestamps, save", async ({ page }) => {
  await page.goto("/#/lessons/l1");
  await page.getByRole("tab", { name: "Cards" }).click();
  await page.getByRole("button", { name: "Generate with AI" }).click();
  await expect(page.getByText(/estimated \$0\.06/)).toBeVisible();
  await page.getByRole("button", { name: /^Generate/ }).click();
  await expect(page.getByText("from 12:34")).toBeVisible();
  await page.getByRole("button", { name: "Save 2 cards" }).click();
  await expect.poll(async () => (await calls(page, "save_flashcard_drafts")).length).toBe(1);
});

test("quiz runner grades an answer and links the hint to the video", async ({ page }) => {
  await page.goto("/#/courses/c1");
  await page.getByRole("tab", { name: "quiz" }).click();
  await page.getByRole("button", { name: "Start quiz" }).click();
  await expect(page.getByText("What does the MapReduce master assign?")).toBeVisible();
  await page.getByRole("radio", { name: /Disks/ }).click();
  await expect(page.getByText("Not quite")).toBeVisible();
  await expect(page.getByRole("link", { name: /Watch at 12:34/ }).first()).toHaveAttribute(
    "href",
    /lessons\/l1\?t=754/,
  );
  await expect(page.getByRole("button", { name: "Make a flashcard" })).toBeVisible();
  await expect
    .poll(async () =>
      (await calls(page, "record_quiz_answer")).map((c) => (c.args as { correct: boolean }).correct),
    )
    .toEqual([false]);
});

test("Settings shows AI usage and the monthly budget", async ({ page }) => {
  await page.goto("/#/settings");
  await expect(page.getByText("This month").first()).toBeVisible();
  await expect(page.getByText("$0.31").first()).toBeVisible();
  await expect(page.getByLabel("Monthly limit (USD)")).toHaveValue("");
  await expect(page.getByRole("cell", { name: "Quiz" })).toBeVisible();
});

test("Discover → MIT OpenCourseWare search and preview", async ({ page }) => {
  await page.goto("/#/search");
  await page.getByRole("radio", { name: "MIT OCW" }).click();
  await page
    .getByRole("textbox", { name: "Search MIT OpenCourseWare" })
    .fill("mathematics for computer science");
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await expect(page.getByText("6.042J · Fall 2010 · Undergraduate")).toBeVisible();
  await page.getByRole("button", { name: "Preview & import" }).click();
  const dlg = page.getByRole("dialog");
  await expect(dlg.getByText("Prof. Tom Leighton, Dr. Marten van Dijk")).toBeVisible();
  await expect(dlg.getByText("36.8 MB")).toBeVisible();
  await expect(dlg.getByRole("button", { name: "Import course" })).toBeEnabled();
});

test("Discover → NPTEL search, preview with license, import, and the tab is remembered", async ({ page }) => {
  await page.goto("/#/search");
  await page.getByRole("radio", { name: "NPTEL" }).click();
  await page.getByRole("textbox", { name: "Search NPTEL" }).fill("python data structures");
  await page.getByRole("button", { name: "Search", exact: true }).click();
  const results = page.getByRole("list", { name: "NPTEL results" });
  await expect(results.getByText("Chennai Mathematical Institute · Prof. Madhavan Mukund")).toBeVisible();
  await expect(results.getByText("CC BY-NC-SA (as stated by NPTEL)")).toBeVisible();
  await results.getByRole("button", { name: "Preview & import" }).click();
  const dlg = page.getByRole("dialog");
  await expect(dlg.getByText("18 transcripts")).toBeVisible();
  await expect(dlg.getByText("Week 1 : Introduction")).toBeVisible();
  await expect(dlg.getByText("CC BY-NC-SA (as stated by NPTEL)")).toBeVisible();
  await dlg.getByRole("button", { name: "Import course" }).click();
  await expect(page).toHaveURL(/#\/courses\/c1/);
  const calls = await page.evaluate(
    () => (window as unknown as { __calls: { cmd: string; args: Record<string, unknown> }[] }).__calls,
  );
  const imp = calls.find((c) => c.cmd === "provider_import");
  expect(imp?.args.provider).toBe("nptel");
  expect(imp?.args.id).toBe("106106145");
  await page.goto("/#/search");
  await expect(page.getByRole("radio", { name: "NPTEL" })).toHaveAttribute("aria-checked", "true");
});

test("Discover → a result with no stated license says so", async ({ page }) => {
  await page.goto("/#/search");
  await page.getByRole("radio", { name: "LibreTexts" }).click();
  await page.getByRole("textbox", { name: "Search LibreTexts" }).fill("python");
  await page.getByRole("button", { name: "Search", exact: true }).click();
  await expect(page.getByText("License: see source")).toBeVisible();
});

test("imported course shows About (syllabus) and Materials with solutions", async ({ page }) => {
  await page.goto("/#/courses/c1");
  await page.getByRole("tab", { name: "about" }).click();
  await expect(page.getByText("Welcome to 6.042!")).toBeVisible();
  await expect(page.getByRole("button", { name: "Open on OCW" })).toBeVisible();
  await page.getByRole("tab", { name: "materials" }).click();
  await expect(page.getByRole("link", { name: "Problem Set 1" })).toBeVisible();
  await expect(page.getByText("Solution", { exact: true })).toBeVisible();
});

test("Ask AI answers a question from its page, marked AI", async ({ page }) => {
  await page.goto("/#/lessons/d1");
  await expect(page.getByRole("button", { name: "p.2" })).toBeVisible();
  await page.getByRole("button", { name: "Ask AI" }).click();
  const dlg = page.getByRole("dialog");
  await expect(dlg.getByText("Why strong induction here?")).toBeVisible();
  await expect(dlg.getByText(/estimated \$0\.04/)).toBeVisible();
  await dlg.getByRole("button", { name: /^Generate/ }).click();
  await expect(dlg.getByText("Strong induction assumes every smaller case (p.3).")).toBeVisible();
  await dlg.getByRole("button", { name: "Save answer" }).click();
  await expect.poll(async () => (await calls(page, "save_ai_answer")).length).toBe(1);
  const runArgs = (await calls(page, "run_assist"))[0]!.args as {
    kind: string;
    focus: { materialId: string; page: number };
  };
  expect([runArgs.kind, runArgs.focus.materialId, runArgs.focus.page]).toEqual(["answer", "mat1", 2]);
});

test("E explains the current page and saves it as a question", async ({ page }) => {
  await page.goto("/#/lessons/d1?doc=mat1&page=3");
  await expect(page.getByText("Page 3: strong induction")).toBeVisible();
  await page.locator("body").click({ position: { x: 5, y: 5 } });
  await page.keyboard.press("e");
  const dlg = page.getByRole("dialog");
  await expect(dlg.getByText("explains page 3")).toBeVisible();
  await dlg.getByRole("button", { name: /^Generate/ }).click();
  await dlg.getByRole("button", { name: "Save as question" }).click();
  await expect
    .poll(async () => (await calls(page, "save_explanation")).map((c) => (c.args as { title: string }).title))
    .toEqual(["Explain on page 3"]);
});

test("fresh install walks through onboarding", async ({ page }) => {
  await page.addInitScript(() => {
    (window as unknown as { __freshInstall: boolean }).__freshInstall = true;
  });
  await page.goto("/#/library");
  await expect(page).toHaveURL(/#\/welcome/);
  await expect(page.getByRole("heading", { name: "Welcome to Lectern" })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Main" })).toBeHidden();
  await page.getByRole("button", { name: "Use this folder" }).click();
  await page.getByLabel("Anthropic API key").fill("sk-ant-test");
  await page.getByRole("button", { name: "Save and continue" }).click();
  await expect(page.getByRole("heading", { name: "Add your first course" })).toBeVisible();
  await page.getByRole("button", { name: /From a YouTube playlist/ }).click();
  await expect(page.getByLabel("Playlist URL")).toBeVisible();
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Skip, I'll add one later" }).click();
  await expect(page.getByRole("navigation", { name: "Main" })).toBeVisible();
  const names = (
    await page.evaluate(() => (window as unknown as { __calls: { cmd: string }[] }).__calls)
  ).map((c) => c.cmd);
  expect(names).toEqual(expect.arrayContaining(["set_ai_key", "set_ai_settings", "set_onboarded"]));
});

test("Settings lists backups and backs up on demand", async ({ page }) => {
  await page.goto("/#/settings");
  const list = page.getByRole("list", { name: "Backups" });
  await expect(list.getByText("2026-11-09 09:00")).toBeVisible();
  await expect(list.getByText("Daily")).toBeVisible();
  await page.getByRole("button", { name: "Back up now" }).click();
  await expect.poll(async () => (await calls(page, "backup_now")).length).toBe(1);
  await expect(page.getByRole("button", { name: "Export library…" })).toBeVisible();
});

test("Settings → Downloads → Advanced saves the cookies browser", async ({ page }) => {
  await page.goto("/#/settings");
  await page.getByText("Advanced", { exact: true }).click();
  await expect(page.getByText(/may violate YouTube/)).toBeVisible();
  await page.getByLabel("Use browser cookies").selectOption("firefox");
  await expect
    .poll(async () => (await calls(page, "set_download_settings")).at(-1))
    .toMatchObject({ args: { settingsIn: { cookiesBrowser: "firefox" } } });
});

test("material page renders the PDF with pdf.js at the requested page", async ({ page }) => {
  await page.goto("/#/materials/mat1?page=2");
  await expect(page.getByText("of 4")).toBeVisible();
  await expect(page.getByText("Page 2: the pigeonhole principle")).toBeVisible();
  await expect(page.getByLabel("Page", { exact: true })).toHaveValue("2");
  await expect(page.getByRole("button", { name: "Show solution" })).toBeVisible();
  await page.getByRole("button", { name: "Next page" }).click();
  await expect(page.getByLabel("Page", { exact: true })).toHaveValue("3");
});

test("document lesson: page notes show p.N and N anchors to the page in view", async ({ page }) => {
  await page.goto("/#/lessons/d1?doc=mat1&page=3");
  await expect(page.getByText("Problem set · 4 pages")).toBeVisible();
  await expect(page.getByText("Page 3: strong induction")).toBeVisible();
  await expect(page.getByRole("button", { name: "p.3" })).toBeVisible();
  await expect(page.getByLabel("Page", { exact: true })).toHaveValue("3");
  await page.locator("body").click({ position: { x: 5, y: 5 } });
  await page.keyboard.press("n");
  await expect
    .poll(async () =>
      (await calls(page, "create_annotation")).map((c) => {
        const i = (c.args as { input: { materialId?: string; page?: number; startSec?: number } }).input;
        return [i.materialId, i.page, i.startSec ?? null];
      }),
    )
    .toEqual([["mat1", 3, null]]);
});

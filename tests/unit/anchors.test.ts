import { describe, expect, it } from "vitest";
import { anchorLabel, anchorSearch, cardAnchor, compareAnchors } from "@/lib/anchors";

describe("anchors", () => {
  it("labels video moments and PDF pages", () => {
    expect(anchorLabel({ startSec: 754 })).toBe("12:34");
    expect(anchorLabel({ materialId: "m1", page: 3 })).toBe("p.3");
    expect(anchorLabel({})).toBeNull();
  });

  it("builds lesson search params", () => {
    expect(anchorSearch({ startSec: 75.6 })).toEqual({ t: 75 });
    expect(anchorSearch({ materialId: "m1", page: 3 })).toEqual({ doc: "m1", page: 3 });
    expect(anchorSearch({ page: 3 })).toEqual({});
  });

  it("orders by time, then page, unanchored last", () => {
    const items = [{ page: 4 }, {}, { startSec: 30 }, { page: 1 }, { startSec: 5 }];
    expect(items.sort(compareAnchors)).toEqual([
      { startSec: 5 },
      { startSec: 30 },
      { page: 1 },
      { page: 4 },
      {},
    ]);
  });

  it("maps card sources", () => {
    expect(cardAnchor({ sourceSec: null, sourceMaterialId: "m", sourcePage: 2 })).toEqual({
      startSec: null,
      materialId: "m",
      page: 2,
    });
  });
});

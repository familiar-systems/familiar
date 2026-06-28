import { describe, expect, it } from "vitest";
import { parseTrans } from "./Trans";

describe("parseTrans", () => {
  it("returns one text segment when there are no tags", () => {
    expect(parseTrans("Just words.")).toEqual([{ kind: "text", text: "Just words." }]);
  });

  it("returns an empty array for an empty message", () => {
    expect(parseTrans("")).toEqual([]);
  });

  it("splits text around a single tag", () => {
    expect(parseTrans("Your <gold>worlds</gold> await.")).toEqual([
      { kind: "text", text: "Your " },
      { kind: "tag", name: "gold", inner: "worlds" },
      { kind: "text", text: " await." },
    ]);
  });

  it("handles a leading tag", () => {
    expect(parseTrans("<b>Never</b> used to train.")).toEqual([
      { kind: "tag", name: "b", inner: "Never" },
      { kind: "text", text: " used to train." },
    ]);
  });

  it("handles a trailing tag", () => {
    expect(parseTrans("Choose your <gold>system</gold>")).toEqual([
      { kind: "text", text: "Choose your " },
      { kind: "tag", name: "gold", inner: "system" },
    ]);
  });

  it("matches two sibling tags independently", () => {
    expect(parseTrans("Data is <b>never</b> sold or <b>shared</b>.")).toEqual([
      { kind: "text", text: "Data is " },
      { kind: "tag", name: "b", inner: "never" },
      { kind: "text", text: " sold or " },
      { kind: "tag", name: "b", inner: "shared" },
      { kind: "text", text: "." },
    ]);
  });

  it("handles adjacent tags with no text between", () => {
    expect(parseTrans("<b>A</b><i>B</i>")).toEqual([
      { kind: "tag", name: "b", inner: "A" },
      { kind: "tag", name: "i", inner: "B" },
    ]);
  });

  it("leaves an unclosed tag as literal text (the Trans dev guard warns)", () => {
    expect(parseTrans("Unclosed <gold>here")).toEqual([
      { kind: "text", text: "Unclosed <gold>here" },
    ]);
  });
});

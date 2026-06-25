import { describe, expect, it } from "vitest";
import { escapeHtml, highlight, langFromPath } from "./DiffView";

describe("diff highlighting helpers", () => {
  it("escapes script-shaped content when no language is known", () => {
    expect(escapeHtml(`<script>alert("x")</script> & more`)).toBe(
      `&lt;script&gt;alert("x")&lt;/script&gt; &amp; more`,
    );
    expect(highlight(`<script>alert("x")</script>`, undefined)).toBe(
      `&lt;script&gt;alert("x")&lt;/script&gt;`,
    );
  });

  it("highlights known languages without throwing", () => {
    const lang = langFromPath("src/main.ts");

    expect(lang).toBe("typescript");
    expect(highlight("const answer = 42;", lang)).toContain("answer");
    expect(highlight(`<script>alert("x")</script>`, lang)).not.toContain("<script>");
  });

  it("renders empty content as a visible placeholder", () => {
    expect(highlight("", undefined)).toBe("&nbsp;");
  });
});

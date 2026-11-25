import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { isAlreadyExistsError, suggestAvailableName } from "./name-conflict.ts";

describe("name conflict helpers", () => {
  it("recognizes daemon create-only conflict errors", () => {
    assert.equal(
      isAlreadyExistsError("[skill_already_exists] a skill named `review` already exists"),
      true,
    );
    assert.equal(
      isAlreadyExistsError(new Error("An MCP server named `github` already exists in this scope")),
      true,
    );
    assert.equal(isAlreadyExistsError("[skill_etag_conflict] modified concurrently"), false);
    assert.equal(isAlreadyExistsError(new Error("host is disconnected")), false);
  });

  it("suggests the first free numbered variant", () => {
    assert.equal(suggestAvailableName("review", ["review"]), "review-2");
    assert.equal(suggestAvailableName("review", ["review", "review-2"]), "review-3");
    assert.equal(suggestAvailableName("review", []), "review-2");
  });

  it("advances an existing counter instead of stacking suffixes", () => {
    assert.equal(suggestAvailableName("review-2", ["review", "review-2"]), "review-3");
    assert.equal(suggestAvailableName("review-9", ["review-9"]), "review-10");
  });

  it("compares case-insensitively and falls back when the range is exhausted", () => {
    assert.equal(suggestAvailableName("Review", ["review-2"]), "Review-3");
    assert.equal(suggestAvailableName("x", ["x-2", "x-3"], 2), "x");
  });
});

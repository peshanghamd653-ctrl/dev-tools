import { describe, expect, it } from "vitest";
import {
  base64Decode,
  base64Encode,
  decodeJwt,
  formatJson,
  hashText,
  minifyJson,
  testRegex,
  urlDecode,
  urlEncode,
} from "./utils";

describe("formatJson / minifyJson", () => {
  it("pretty-prints valid JSON", () => {
    const result = formatJson('{"b":2,"a":1}');
    expect(result).toEqual({ ok: true, result: '{\n  "b": 2,\n  "a": 1\n}' });
  });

  it("reports the parse error rather than throwing", () => {
    const result = formatJson("{not json");
    expect(result.ok).toBe(false);
  });

  it("minifies whitespace", () => {
    expect(minifyJson('{\n  "a": 1\n}')).toEqual({
      ok: true,
      result: '{"a":1}',
    });
  });
});

describe("base64Encode / base64Decode", () => {
  it("round-trips plain ASCII", () => {
    const encoded = base64Encode("hello world");
    expect(base64Decode(encoded)).toEqual({ ok: true, result: "hello world" });
  });

  it("round-trips UTF-8 outside Latin-1 — plain btoa would throw here", () => {
    const encoded = base64Encode("héllo 世界 🚀");
    expect(base64Decode(encoded)).toEqual({
      ok: true,
      result: "héllo 世界 🚀",
    });
  });

  it("reports invalid base64 rather than throwing", () => {
    const result = base64Decode("not valid base64!!!");
    expect(result.ok).toBe(false);
  });
});

describe("urlEncode / urlDecode", () => {
  it("round-trips a string with reserved characters", () => {
    const encoded = urlEncode("a b/c?d=e&f");
    expect(urlDecode(encoded)).toEqual({ ok: true, result: "a b/c?d=e&f" });
  });

  it("reports a malformed percent-sequence rather than throwing", () => {
    const result = urlDecode("100% not valid %");
    expect(result.ok).toBe(false);
  });
});

describe("decodeJwt", () => {
  // header {"alg":"HS256","typ":"JWT"}, payload {"sub":"1234567890","name":"John Doe"}
  const token =
    "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";

  it("decodes the header and payload without verifying the signature", () => {
    const result = decodeJwt(token);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(JSON.parse(result.value.header)).toEqual({
      alg: "HS256",
      typ: "JWT",
    });
    expect(JSON.parse(result.value.payload)).toEqual({
      sub: "1234567890",
      name: "John Doe",
    });
  });

  it("rejects a string that is not three dot-separated parts", () => {
    expect(decodeJwt("not.a.jwt.at.all").ok).toBe(false);
    expect(decodeJwt("onlyonepart").ok).toBe(false);
  });

  it("rejects a part that decodes to non-JSON", () => {
    expect(decodeJwt("bm90anNvbg.bm90anNvbg.sig").ok).toBe(false);
  });
});

describe("hashText", () => {
  it("matches the known SHA-256 digest of an empty string", async () => {
    const digest = await hashText("SHA-256", "");
    expect(digest).toBe(
      "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
  });

  it("matches the known SHA-1 digest of 'abc'", async () => {
    const digest = await hashText("SHA-1", "abc");
    expect(digest).toBe("a9993e364706816aba3e25717850c26c9cd0d89d");
  });
});

describe("testRegex", () => {
  it("finds every match with its position", () => {
    const result = testRegex("\\d+", "", "a12b345c");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.matches).toEqual([
      { match: "12", index: 1, groups: [] },
      { match: "345", index: 4, groups: [] },
    ]);
  });

  it("captures groups", () => {
    const result = testRegex("(\\w+)@(\\w+)", "", "user@host");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.matches[0]!.groups).toEqual(["user", "host"]);
  });

  it("reports an invalid pattern rather than throwing", () => {
    const result = testRegex("(unclosed", "", "text");
    expect(result.ok).toBe(false);
  });

  it("finds every match even when the user didn't pass the g flag", () => {
    const result = testRegex("a", "i", "aAaA");
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.matches).toHaveLength(4);
  });
});

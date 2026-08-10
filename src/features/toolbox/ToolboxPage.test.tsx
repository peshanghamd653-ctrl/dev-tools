/**
 * `utils.test.ts` already covers the transforms themselves. What matters
 * here is the wiring: picking a tool shows its panel, a button calls the
 * right transform, and an error result renders instead of a result pane.
 * No IPC mock is needed — every tool in this page runs entirely client-side.
 */
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { ToolboxPage } from "./ToolboxPage";

afterEach(cleanup);

describe("ToolboxPage navigation", () => {
  it("starts on the JSON formatter", () => {
    render(<ToolboxPage />);
    expect(
      screen.getByRole("heading", { name: "JSON Formatter" }),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Input")).toBeInTheDocument();
  });

  it("switches panels when a different tool is selected", () => {
    render(<ToolboxPage />);
    fireEvent.click(screen.getByRole("button", { name: "Base64" }));

    expect(
      screen.getByText("Encode or decode base64, UTF-8 safe."),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("Token")).not.toBeInTheDocument();
  });
});

describe("JSON formatter", () => {
  it("formats valid JSON", () => {
    render(<ToolboxPage />);
    fireEvent.change(screen.getByLabelText("Input"), {
      target: { value: '{"b":2,"a":1}' },
    });
    fireEvent.click(screen.getByRole("button", { name: "Format" }));

    expect(screen.getByLabelText("Result")).toHaveValue(
      '{\n  "b": 2,\n  "a": 1\n}',
    );
  });

  it("shows the parse error instead of a result", () => {
    render(<ToolboxPage />);
    fireEvent.change(screen.getByLabelText("Input"), {
      target: { value: "{not json" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Format" }));

    expect(screen.queryByLabelText("Result")).not.toBeInTheDocument();
    expect(
      screen.getByText(/Expected property name|Unexpected token/),
    ).toBeInTheDocument();
  });
});

describe("Base64 tool", () => {
  it("round-trips through the Encode and Decode buttons", () => {
    render(<ToolboxPage />);
    fireEvent.click(screen.getByRole("button", { name: "Base64" }));

    fireEvent.change(screen.getByLabelText("Input"), {
      target: { value: "hello" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Encode" }));
    const encoded = screen.getByLabelText("Result") as HTMLTextAreaElement;
    expect(encoded.value).toBe("aGVsbG8=");

    fireEvent.change(screen.getByLabelText("Input"), {
      target: { value: encoded.value },
    });
    fireEvent.click(screen.getByRole("button", { name: "Decode" }));
    expect(screen.getByLabelText("Result")).toHaveValue("hello");
  });
});

describe("UUID generator", () => {
  it("prepends a new UUID to the list on each click", () => {
    render(<ToolboxPage />);
    fireEvent.click(screen.getByRole("button", { name: "UUID Generator" }));

    fireEvent.click(screen.getByRole("button", { name: "Generate" }));
    const list = () =>
      within(screen.getByRole("list", { name: "Generated UUIDs" }));
    expect(list().getAllByRole("listitem")).toHaveLength(1);

    fireEvent.click(screen.getByRole("button", { name: "Generate" }));
    expect(list().getAllByRole("listitem")).toHaveLength(2);
  });
});

describe("Regex tester", () => {
  it("reports match count and positions as the user types", () => {
    render(<ToolboxPage />);
    fireEvent.click(screen.getByRole("button", { name: "Regex Tester" }));

    fireEvent.change(screen.getByLabelText("Pattern"), {
      target: { value: "\\d+" },
    });
    fireEvent.change(screen.getByLabelText("Text"), {
      target: { value: "a12b345c" },
    });

    expect(screen.getByText("2 matches")).toBeInTheDocument();
    const matches = within(screen.getByRole("list", { name: "Matches" }));
    expect(matches.getByText(/12/)).toBeInTheDocument();
    expect(matches.getByText(/345/)).toBeInTheDocument();
  });

  it("shows the regex error rather than a match count", () => {
    render(<ToolboxPage />);
    fireEvent.click(screen.getByRole("button", { name: "Regex Tester" }));

    fireEvent.change(screen.getByLabelText("Pattern"), {
      target: { value: "(unclosed" },
    });

    expect(screen.queryByText(/matches?$/)).not.toBeInTheDocument();
  });
});

describe("JSON to YAML", () => {
  it("converts JSON to YAML", () => {
    render(<ToolboxPage />);
    fireEvent.click(screen.getByRole("button", { name: "JSON to YAML" }));

    fireEvent.change(screen.getByLabelText("Input"), {
      target: { value: '{"a":1}' },
    });
    fireEvent.click(screen.getByRole("button", { name: "Convert" }));

    expect(screen.getByLabelText("Result")).toHaveValue("a: 1");
  });
});

describe("XML formatter", () => {
  it("pretty-prints valid XML", () => {
    render(<ToolboxPage />);
    fireEvent.click(screen.getByRole("button", { name: "XML Formatter" }));

    fireEvent.change(screen.getByLabelText("Input"), {
      target: { value: "<a><b>1</b></a>" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Format" }));

    expect(screen.getByLabelText("Result")).toHaveValue(
      "<a>\n  <b>1</b>\n</a>",
    );
  });

  it("shows a parse error instead of a result", () => {
    render(<ToolboxPage />);
    fireEvent.click(screen.getByRole("button", { name: "XML Formatter" }));

    fireEvent.change(screen.getByLabelText("Input"), {
      target: { value: "<a><b></a>" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Format" }));

    expect(screen.queryByLabelText("Result")).not.toBeInTheDocument();
  });
});

describe("Markdown preview", () => {
  it("renders Markdown as HTML while typing", () => {
    render(<ToolboxPage />);
    fireEvent.click(screen.getByRole("button", { name: "Markdown Preview" }));

    fireEvent.change(screen.getByLabelText("Input"), {
      target: { value: "# Hello" },
    });

    expect(
      screen.getByRole("heading", { level: 1, name: "Hello" }),
    ).toBeInTheDocument();
  });
});

describe("Text/JSON diff", () => {
  it("shows added and removed lines", () => {
    render(<ToolboxPage />);
    fireEvent.click(screen.getByRole("button", { name: "Text/JSON Diff" }));

    fireEvent.change(screen.getByLabelText("Original"), {
      target: { value: "a\nb" },
    });
    fireEvent.change(screen.getByLabelText("Changed"), {
      target: { value: "a\nc" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Compare" }));

    const diff = within(screen.getByLabelText("Diff result"));
    expect(diff.getByText(/-\s*b/)).toBeInTheDocument();
    expect(diff.getByText(/\+\s*c/)).toBeInTheDocument();
  });
});

describe("HTTP status lookup", () => {
  it("filters as the user types", () => {
    render(<ToolboxPage />);
    fireEvent.click(screen.getByRole("button", { name: "HTTP Status Lookup" }));

    const list = () =>
      within(screen.getByRole("list", { name: "HTTP status codes" }));
    expect(list().getAllByRole("listitem").length).toBeGreaterThan(10);

    fireEvent.change(screen.getByLabelText("Search"), {
      target: { value: "404" },
    });
    expect(list().getAllByRole("listitem")).toHaveLength(1);
    expect(list().getByText("Not Found")).toBeInTheDocument();
  });
});

describe("JWT decoder", () => {
  it("decodes the header and payload of a real token", () => {
    render(<ToolboxPage />);
    fireEvent.click(screen.getByRole("button", { name: "JWT Decoder" }));

    fireEvent.change(screen.getByLabelText("Token"), {
      target: {
        value:
          "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
      },
    });
    fireEvent.click(screen.getByRole("button", { name: "Decode" }));

    expect(screen.getByLabelText("Header")).toHaveValue(
      '{\n  "alg": "HS256",\n  "typ": "JWT"\n}',
    );
    expect(screen.getByLabelText("Payload")).toHaveTextContent("John Doe");
  });

  it("explains a malformed token instead of crashing", () => {
    render(<ToolboxPage />);
    fireEvent.click(screen.getByRole("button", { name: "JWT Decoder" }));

    fireEvent.change(screen.getByLabelText("Token"), {
      target: { value: "not-a-jwt" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Decode" }));

    expect(screen.getByText(/three dot-separated parts/)).toBeInTheDocument();
  });
});

import { useState } from "react";
import {
  Braces,
  Copy,
  FileKey,
  Hash,
  Link2,
  Regex,
  Wrench,
} from "lucide-react";
import { toast } from "sonner";

import { cn } from "@/shared/lib/utils";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Label } from "@/shared/ui/label";
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
  type HashAlgorithm,
  type ToolResult,
} from "./utils";

type ToolId = "json" | "base64" | "url" | "uuid" | "hash" | "jwt" | "regex";

const TOOLS: { id: ToolId; label: string; icon: typeof Braces }[] = [
  { id: "json", label: "JSON Formatter", icon: Braces },
  { id: "base64", label: "Base64", icon: FileKey },
  { id: "url", label: "URL Encode/Decode", icon: Link2 },
  { id: "uuid", label: "UUID Generator", icon: Wrench },
  { id: "hash", label: "Hash Generator", icon: Hash },
  { id: "jwt", label: "JWT Decoder", icon: FileKey },
  { id: "regex", label: "Regex Tester", icon: Regex },
];

/**
 * A small, self-contained collection of everyday text transforms — not a
 * destination in its own right, and deliberately kept that way. Every tool
 * here runs entirely in the webview: no IPC, no file access, nothing that
 * could touch a project. Paste in, get an answer, move on.
 */
export function ToolboxPage() {
  const [active, setActive] = useState<ToolId>("json");

  return (
    <div className="grid h-full grid-cols-[220px_1fr]">
      <aside className="min-h-0 overflow-y-auto border-r p-2">
        <ul className="space-y-0.5" aria-label="Tools">
          {TOOLS.map((tool) => (
            <li key={tool.id}>
              <button
                type="button"
                onClick={() => setActive(tool.id)}
                className={cn(
                  "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm",
                  active === tool.id
                    ? "bg-accent text-foreground"
                    : "text-muted-foreground hover:bg-accent/60",
                )}
              >
                <tool.icon className="size-4 shrink-0" />
                <span className="truncate">{tool.label}</span>
              </button>
            </li>
          ))}
        </ul>
      </aside>

      <section className="min-h-0 overflow-y-auto p-6">
        {active === "json" && <JsonFormatterTool />}
        {active === "base64" && <Base64Tool />}
        {active === "url" && <UrlEncodeTool />}
        {active === "uuid" && <UuidTool />}
        {active === "hash" && <HashTool />}
        {active === "jwt" && <JwtDecoderTool />}
        {active === "regex" && <RegexTesterTool />}
      </section>
    </div>
  );
}

function ToolShell({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: React.ReactNode;
}) {
  return (
    <div className="mx-auto max-w-2xl space-y-4">
      <div>
        <h1 className="text-lg font-semibold tracking-tight">{title}</h1>
        <p className="text-sm text-muted-foreground">{description}</p>
      </div>
      {children}
    </div>
  );
}

function TextArea(props: React.TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return (
    <textarea
      {...props}
      className={cn(
        "w-full resize-y rounded-md border bg-transparent px-2.5 py-1.5 font-mono text-xs outline-none placeholder:text-muted-foreground focus-visible:border-ring",
        props.className,
      )}
    />
  );
}

function CopyButton({ value }: { value: string }) {
  return (
    <Button
      variant="ghost"
      size="icon"
      className="size-7"
      aria-label="Copy result"
      disabled={!value}
      onClick={() =>
        void navigator.clipboard
          .writeText(value)
          .then(() => toast.success("Copied"))
      }
    >
      <Copy className="size-3.5" />
    </Button>
  );
}

function ResultPane({ result }: { result: ToolResult | null }) {
  if (!result) return null;
  if (!result.ok) {
    return (
      <p className="rounded-md border border-destructive/40 bg-destructive/10 px-2.5 py-1.5 text-xs text-red-300">
        {result.error}
      </p>
    );
  }
  return (
    <div className="space-y-1.5">
      <div className="flex items-center justify-between">
        <Label htmlFor="tool-result">Result</Label>
        <CopyButton value={result.result} />
      </div>
      <TextArea id="tool-result" value={result.result} readOnly rows={8} />
    </div>
  );
}

function JsonFormatterTool() {
  const [input, setInput] = useState("");
  const [result, setResult] = useState<ToolResult | null>(null);

  return (
    <ToolShell
      title="JSON Formatter"
      description="Pretty-print, minify, and validate JSON."
    >
      <div className="space-y-1.5">
        <Label htmlFor="json-input">Input</Label>
        <TextArea
          id="json-input"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder='{"key": "value"}'
          rows={8}
        />
      </div>
      <div className="flex gap-2">
        <Button size="sm" onClick={() => setResult(formatJson(input))}>
          Format
        </Button>
        <Button
          size="sm"
          variant="outline"
          onClick={() => setResult(minifyJson(input))}
        >
          Minify
        </Button>
      </div>
      <ResultPane result={result} />
    </ToolShell>
  );
}

function Base64Tool() {
  const [input, setInput] = useState("");
  const [result, setResult] = useState<ToolResult | null>(null);

  return (
    <ToolShell
      title="Base64"
      description="Encode or decode base64, UTF-8 safe."
    >
      <div className="space-y-1.5">
        <Label htmlFor="b64-input">Input</Label>
        <TextArea
          id="b64-input"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          rows={5}
        />
      </div>
      <div className="flex gap-2">
        <Button
          size="sm"
          onClick={() => setResult({ ok: true, result: base64Encode(input) })}
        >
          Encode
        </Button>
        <Button
          size="sm"
          variant="outline"
          onClick={() => setResult(base64Decode(input))}
        >
          Decode
        </Button>
      </div>
      <ResultPane result={result} />
    </ToolShell>
  );
}

function UrlEncodeTool() {
  const [input, setInput] = useState("");
  const [result, setResult] = useState<ToolResult | null>(null);

  return (
    <ToolShell
      title="URL Encode/Decode"
      description="Percent-encode or decode a string or query value."
    >
      <div className="space-y-1.5">
        <Label htmlFor="url-input">Input</Label>
        <TextArea
          id="url-input"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          rows={5}
        />
      </div>
      <div className="flex gap-2">
        <Button
          size="sm"
          onClick={() => setResult({ ok: true, result: urlEncode(input) })}
        >
          Encode
        </Button>
        <Button
          size="sm"
          variant="outline"
          onClick={() => setResult(urlDecode(input))}
        >
          Decode
        </Button>
      </div>
      <ResultPane result={result} />
    </ToolShell>
  );
}

function UuidTool() {
  const [uuids, setUuids] = useState<string[]>([]);

  return (
    <ToolShell title="UUID Generator" description="Generate random (v4) UUIDs.">
      <Button
        size="sm"
        onClick={() => setUuids((prev) => [crypto.randomUUID(), ...prev])}
      >
        Generate
      </Button>
      {uuids.length > 0 && (
        <ul className="space-y-1" aria-label="Generated UUIDs">
          {uuids.map((uuid) => (
            <li
              key={uuid}
              className="flex items-center justify-between gap-2 rounded-md border px-2.5 py-1.5"
            >
              <span className="truncate font-mono text-xs">{uuid}</span>
              <CopyButton value={uuid} />
            </li>
          ))}
        </ul>
      )}
    </ToolShell>
  );
}

const HASH_ALGORITHMS: HashAlgorithm[] = ["SHA-1", "SHA-256", "SHA-512"];

function HashTool() {
  const [input, setInput] = useState("");
  const [algorithm, setAlgorithm] = useState<HashAlgorithm>("SHA-256");
  const [result, setResult] = useState<string | null>(null);

  return (
    <ToolShell
      title="Hash Generator"
      description="SHA-1, SHA-256, or SHA-512 digest of some text."
    >
      <div className="space-y-1.5">
        <Label htmlFor="hash-input">Input</Label>
        <TextArea
          id="hash-input"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          rows={5}
        />
      </div>
      <div className="flex items-center gap-2">
        <select
          value={algorithm}
          onChange={(e) => setAlgorithm(e.target.value as HashAlgorithm)}
          className="h-9 rounded-md border bg-transparent px-2 text-xs outline-none focus-visible:border-ring"
        >
          {HASH_ALGORITHMS.map((algo) => (
            <option key={algo} value={algo} className="bg-popover">
              {algo}
            </option>
          ))}
        </select>
        <Button
          size="sm"
          onClick={() => void hashText(algorithm, input).then(setResult)}
        >
          Generate
        </Button>
      </div>
      {result !== null && (
        <div className="flex items-center justify-between gap-2 rounded-md border bg-card px-2.5 py-1.5">
          <span className="min-w-0 truncate font-mono text-xs">{result}</span>
          <CopyButton value={result} />
        </div>
      )}
    </ToolShell>
  );
}

function JwtDecoderTool() {
  const [input, setInput] = useState("");
  const [decoded, setDecoded] = useState<
    | { ok: true; header: string; payload: string }
    | { ok: false; error: string }
    | null
  >(null);

  function decode() {
    const result = decodeJwt(input);
    setDecoded(
      result.ok
        ? {
            ok: true,
            header: result.value.header,
            payload: result.value.payload,
          }
        : { ok: false, error: result.error },
    );
  }

  return (
    <ToolShell
      title="JWT Decoder"
      description="Reads a token's header and payload. Never verifies the signature — that needs the issuer's key, which this doesn't have."
    >
      <div className="space-y-1.5">
        <Label htmlFor="jwt-input">Token</Label>
        <TextArea
          id="jwt-input"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          placeholder="eyJhbGciOi...header.eyJzdWIiOi...payload.signature"
          rows={4}
        />
      </div>
      <Button size="sm" onClick={decode}>
        Decode
      </Button>
      {decoded && !decoded.ok && (
        <p className="rounded-md border border-destructive/40 bg-destructive/10 px-2.5 py-1.5 text-xs text-red-300">
          {decoded.error}
        </p>
      )}
      {decoded?.ok && (
        <div className="space-y-3">
          <div className="space-y-1.5">
            <Label htmlFor="jwt-header">Header</Label>
            <TextArea
              id="jwt-header"
              value={decoded.header}
              readOnly
              rows={3}
            />
          </div>
          <div className="space-y-1.5">
            <Label htmlFor="jwt-payload">Payload</Label>
            <TextArea
              id="jwt-payload"
              value={decoded.payload}
              readOnly
              rows={6}
            />
          </div>
        </div>
      )}
    </ToolShell>
  );
}

function RegexTesterTool() {
  const [pattern, setPattern] = useState("");
  const [flags, setFlags] = useState("g");
  const [text, setText] = useState("");

  const result = pattern ? testRegex(pattern, flags, text) : null;

  return (
    <ToolShell
      title="Regex Tester"
      description="JavaScript regular expressions, tested against a block of text."
    >
      <div className="flex gap-2">
        <div className="flex-1 space-y-1.5">
          <Label htmlFor="regex-pattern">Pattern</Label>
          <Input
            id="regex-pattern"
            value={pattern}
            onChange={(e) => setPattern(e.target.value)}
            placeholder="\d+"
            className="font-mono text-xs"
          />
        </div>
        <div className="w-24 space-y-1.5">
          <Label htmlFor="regex-flags">Flags</Label>
          <Input
            id="regex-flags"
            value={flags}
            onChange={(e) => setFlags(e.target.value)}
            className="font-mono text-xs"
          />
        </div>
      </div>
      <div className="space-y-1.5">
        <Label htmlFor="regex-text">Text</Label>
        <TextArea
          id="regex-text"
          value={text}
          onChange={(e) => setText(e.target.value)}
          rows={6}
        />
      </div>
      {result && !result.ok && (
        <p className="rounded-md border border-destructive/40 bg-destructive/10 px-2.5 py-1.5 text-xs text-red-300">
          {result.error}
        </p>
      )}
      {result?.ok && (
        <div className="space-y-1.5">
          <Label>
            {result.matches.length} match
            {result.matches.length === 1 ? "" : "es"}
          </Label>
          {result.matches.length > 0 && (
            <ul className="space-y-1" aria-label="Matches">
              {result.matches.map((m, i) => (
                <li
                  key={i}
                  className="rounded-md border px-2.5 py-1.5 font-mono text-xs"
                >
                  <span className="text-muted-foreground">[{m.index}]</span>{" "}
                  {m.match}
                  {m.groups.length > 0 && (
                    <span className="text-muted-foreground">
                      {" "}
                      — groups: {m.groups.join(", ")}
                    </span>
                  )}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </ToolShell>
  );
}

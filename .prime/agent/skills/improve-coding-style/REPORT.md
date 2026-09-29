# HTML report format

The report is one self-contained HTML file in the OS temp directory. Tailwind comes from its CDN. Code snippets carry the weight, so keep prose short.

## Scaffold

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>Coding style review for {{repo name}}</title>
    <script src="https://cdn.tailwindcss.com"></script>
  </head>
  <body class="bg-stone-50 text-slate-900 font-sans">
    <main class="max-w-6xl mx-auto px-6 py-12 space-y-12">
      <header>...</header>
      <section id="batches" class="space-y-10">...</section>
      <section id="dropped">...</section>
      <section id="top-recommendation">...</section>
    </main>
  </body>
</html>
```

Escape `<`, `>`, and `&` in every code snippet.

## Header

Show the repository name, date, base OID, and scope. Add one row of counts: files reviewed, candidates, findings, dropped, and batches. Add a small table of findings per rubric section. Do not add an introduction paragraph.

## Batch card

Each batch is one `<article>` with these parts:

- **Title.** Name the fix, such as "Separate phases in the manifest writer".
- **Badge row.** Show the strength (`Strong` is emerald, `Worth fixing` is amber, `Borderline` is slate), the risk (`mechanical` or `structural`), and the rule ID in monospace.
- **Rule.** Quote the `Violation` clause that the code meets.
- **Files.** List `path:line-range` entries with `font-mono text-sm`.
- **Before and after.** Show two columns. Put the current code on a red-tinted `<pre>` and the proposed code on a green-tinted `<pre>`. Mark changed lines with a stronger tint. For a batch across many sites, show one representative site and give the count of other sites.
- **Why.** Write one sentence about what the fix makes easier to read or change.
- **Frozen items.** If the fix touches a public API, error or cancellation semantics, or a user-visible message, add an amber callout. That batch needs explicit approval.
- **ADR callout.** If the fix contradicts an ADR, add a one-line amber callout.

## Dropped candidates

Show a compact table with the file, rule ID, source (tool, Jev, or reviewer), and the one-line reason. A dropped Jev candidate is calibration evidence, so mark those rows.

## Top recommendation

Name the batch to fix first and give the reason in two or three sentences. Prefer strong mechanical batches in files that change often, because they are cheap to review and pay off on the next edit.

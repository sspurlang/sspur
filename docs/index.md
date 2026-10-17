<img class="sspur-hero-logo" src="assets/logo/sspur.svg" width="96" height="96" alt="SSPUR logo">

# SSPUR documentation

**A programming language for AI agents to write, read, and maintain.** SSPUR is statically typed and effect-tracked. Code lives in a content-addressed store of typechecked definitions, agents change it through atomic edits that run every test, and contracts are checked at run time, fuzzed and proved. It compiles to native code that runs faster than idiomatic C++ with the same safety checks.

```sspur
{{#include snippets/get-started/cart.ssp}}
```

```console
{{#include snippets/get-started/cart.out}}
```

[Try it in the playground](play/index.html): it checks and runs SSPUR in your browser, with nothing to install.

[SSPUR Pulse](pulse/index.html) is a live dashboard of the world today, fetched, checked and rendered by SSPUR programs every 6 hours. [How it works](pulse-how-it-works.md) describes the programs behind it.

**[SSPUR packages](https://sspurlang.github.io/sspur-packages/)** is the package registry: search packages, read their API docs, and publish your own from the browser or with `sspur publish`. SSPUR's home page is **[sspurlang.github.io](https://sspurlang.github.io/)**.

<div class="sspur-cards">

<div class="sspur-card">
<h3>Get Started</h3>
<p>Install it, write a first program, and map what you know from other languages.</p>
<ul>
<li><a href="get-started/five-minutes.html">SSPUR in 5 minutes</a></li>
<li><a href="get-started/from-typescript.html">For TypeScript programmers</a></li>
<li><a href="get-started/from-python.html">For Python programmers</a></li>
<li><a href="get-started/from-go-rust.html">For Go and Rust programmers</a></li>
<li><a href="get-started/agents.html">For AI agents</a></li>
<li><a href="get-started/installation.html">Installation</a></li>
<li><a href="play/index.html">Playground</a></li>
<li><a href="pulse/index.html">SSPUR Pulse</a></li>
<li><a href="https://sspurlang.github.io/sspur-packages/">Package registry</a></li>
</ul>
</div>

<div class="sspur-card">
<h3>Handbook</h3>
<p>The language from the start, one idea per page, read in order.</p>
<ul>
<li><a href="handbook/basics.html">The Basics</a></li>
<li><a href="handbook/everyday-types.html">Everyday Types</a></li>
<li><a href="handbook/functions.html">Functions and Lambdas</a></li>
<li><a href="handbook/records-and-sums.html">Records and Sum Types</a></li>
<li><a href="handbook/pattern-matching.html">Pattern Matching</a></li>
<li><a href="handbook/effects-and-errors.html">Effects and Errors</a></li>
<li><a href="handbook/contracts-and-tests.html">Contracts and Tests</a></li>
<li><a href="handbook/generics-and-traits.html">Generics and Traits</a></li>
<li><a href="handbook/collections.html">Collections and Pipelines</a></li>
<li><a href="handbook/concurrency.html">Concurrency</a></li>
<li><a href="handbook/packages.html">Packages</a></li>
</ul>
</div>

<div class="sspur-card">
<h3>Reference</h3>
<p>Everything the compiler implements, by area.</p>
<ul>
<li><a href="reference/language.html">Language reference</a></li>
<li><a href="reference/stdlib.html">Standard library</a></li>
<li><a href="reference/effects.html">Effects</a></li>
<li><a href="reference/errors.html">Error codes</a></li>
<li><a href="reference/cli.html">Command line</a></li>
<li><a href="agent/agent-spec.html">Agent reference</a></li>
<li><a href="reference/schema.html">JSON schemas</a></li>
</ul>
</div>

<div class="sspur-card">
<h3>Tutorials</h3>
<p>Real programs, built and run end to end.</p>
<ul>
<li><a href="tutorials/crud-service.html">Build and deploy a CRUD service</a></li>
<li><a href="tutorials/migrations.html">Migrations and hot swap</a></li>
<li><a href="tutorials/ffi.html">Calling C</a></li>
<li><a href="tutorials/bare-metal.html">Bare-metal hello</a></li>
<li><a href="tutorials/gpu-kernels.html">GPU kernels</a></li>
<li><a href="tutorials/multi-agent.html">A multi-agent codebase</a></li>
</ul>
</div>

<div class="sspur-card">
<h3>Project Configuration</h3>
<p>Manifests, lock files and the knobs on the compiler.</p>
<ul>
<li><a href="config/sspur-toml.html">sspur.toml</a></li>
<li><a href="config/sspur-lock.html">sspur.lock</a></li>
<li><a href="config/build-options.html">Build options</a></li>
<li><a href="config/environment.html">Environment variables</a></li>
</ul>
</div>

<div class="sspur-card">
<h3>Cheat Sheet</h3>
<p>The everyday syntax on one checked page.</p>
<ul>
<li><a href="cheat-sheet.html">Cheat sheet</a></li>
</ul>
</div>

<div class="sspur-card">
<h3>Design and Performance</h3>
<p>Why it works this way, and how fast it is.</p>
<ul>
<li><a href="design/index.html">Design documents</a></li>
<li><a href="adr/index.html">Decision records</a></li>
<li><a href="design/native-benchmarks.html">Native benchmarks</a></li>
<li><a href="design/agent-benchmarks.html">Agent benchmarks</a></li>
</ul>
</div>

<div class="sspur-card">
<h3>Release Notes</h3>
<p>What changed in each version.</p>
<ul>
<li><a href="release-notes.html">Changelog</a></li>
<li><a href="https://github.com/sspurlang/sspur">Source on GitHub</a></li>
</ul>
</div>

</div>

SSPUR is at version 0.14 and under active development. The language and tools are usable, but the syntax and standard library can still change between minor releases. It is designed, written and maintained by [Utkarsha Vardhana](https://github.com/utkarshavardhana), and dual-licensed under MIT and Apache 2.0.

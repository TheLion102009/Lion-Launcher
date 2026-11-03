// Markdown → sicheres HTML für Projektbeschreibungen (Modrinth-Bodies).
//
// Modrinth-Beschreibungen mischen Markdown und rohes HTML (z.B. <center>, <img>, Badges wie
// [![Discord](badge.svg)](https://discord.gg/...)). Der Parser orientiert sich an
// CommonMark/GFM (wie Modrinth selbst): Überschriften, Listen, Zitate, Code, Tabellen,
// Links/Bilder inkl. Referenz-Links, Autolinks und HTML-Blöcke.
// Das Ergebnis wird anschließend per Whitelist bereinigt (keine Skripte, Event-Handler,
// Styles oder gefährlichen URLs).
(function (global) {
    'use strict';

    // ── Hilfen ─────────────────────────────────────────────────────────────────
    function esc(value) {
        return String(value)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;');
    }

    const HTML_ATTR = String.raw`\s+[a-zA-Z_:][a-zA-Z0-9_.:-]*(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s"'=<>\x60]+))?`;
    const HTML_OPEN_TAG = new RegExp(String.raw`^<[a-zA-Z][a-zA-Z0-9-]*(?:${HTML_ATTR})*\s*\/?>`);
    const HTML_CLOSE_TAG = /^<\/[a-zA-Z][a-zA-Z0-9-]*\s*>/;
    const HTML_COMMENT = /^<!--[\s\S]*?-->/;
    const HTML_ONLY_TAG_LINE = new RegExp(
        String.raw`^(?:<[a-zA-Z][a-zA-Z0-9-]*(?:${HTML_ATTR})*\s*\/?>|<\/[a-zA-Z][a-zA-Z0-9-]*\s*>)\s*$`
    );

    // CommonMark HTML-Block-Typ 6: diese Tags beginnen einen HTML-Block
    const BLOCK_HTML_TAGS = new Set([
        'address', 'article', 'aside', 'base', 'basefont', 'blockquote', 'body', 'caption', 'center',
        'col', 'colgroup', 'dd', 'details', 'dialog', 'dir', 'div', 'dl', 'dt', 'fieldset',
        'figcaption', 'figure', 'footer', 'form', 'frame', 'frameset', 'h1', 'h2', 'h3', 'h4', 'h5',
        'h6', 'head', 'header', 'hr', 'html', 'iframe', 'legend', 'li', 'link', 'main', 'menu',
        'menuitem', 'nav', 'noframes', 'ol', 'optgroup', 'option', 'p', 'param', 'search', 'section',
        'summary', 'table', 'tbody', 'td', 'tfoot', 'th', 'thead', 'title', 'tr', 'track', 'ul'
    ]);

    const LIST_ITEM_RE = /^( *)([-*+]|\d{1,9}[.)])([ \t]+|$)(.*)$/;
    const THEMATIC_BREAK_RE = /^ {0,3}([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
    const FENCE_RE = /^( {0,3})(`{3,}|~{3,})(.*)$/;
    const ATX_HEADING_RE = /^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$/;
    const TABLE_DELIMITER_RE = /^ {0,3}\|?[ \t]*:?-+:?[ \t]*(?:\|[ \t]*:?-+:?[ \t]*)*\|?[ \t]*$/;
    const REF_DEF_RE = /^ {0,3}\[([^\]]+)]:[ \t]*<?([^\s>]+)>?(?:[ \t]+(?:"([^"]*)"|'([^']*)'|\(([^)]*)\)))?[ \t]*$/;

    function htmlBlockType(line, inParagraph) {
        if (!/^ {0,3}</.test(line)) return null;
        const s = line.replace(/^ {0,3}/, '');
        if (/^<(pre|script|style|textarea)(?=[\s>]|$)/i.test(s)) {
            return { endRe: /<\/(pre|script|style|textarea)>/i };
        }
        if (s.startsWith('<!--')) return { endRe: /-->/ };
        const m = s.match(/^<\/?([a-zA-Z][a-zA-Z0-9-]*)(?=[\s/>]|$)/);
        if (!m) return null;
        if (BLOCK_HTML_TAGS.has(m[1].toLowerCase())) return { endRe: null };
        // Typ 7: ein einzelnes vollständiges Tag allein in der Zeile (unterbricht keinen Absatz)
        if (!inParagraph && HTML_ONLY_TAG_LINE.test(s)) return { endRe: null };
        return null;
    }

    function isListItem(line) {
        return LIST_ITEM_RE.test(line) && !THEMATIC_BREAK_RE.test(line);
    }

    function startsBlock(line) {
        return /^ {0,3}(?:#{1,6}(?:[ \t]|$)|>|`{3,}|~{3,})/.test(line) ||
            THEMATIC_BREAK_RE.test(line) ||
            isListItem(line) ||
            !!htmlBlockType(line, true);
    }

    function indentOf(line) {
        return line.match(/^ */)[0].length;
    }

    // ── Block-Ebene ────────────────────────────────────────────────────────────
    function parseBlocks(lines, ctx, tight) {
        const out = [];
        let para = [];

        const flushParagraph = () => {
            if (!para.length) return;
            const inner = parseInline(para.join('\n').replace(/\s+$/, ''), ctx);
            out.push(tight ? inner : `<p>${inner}</p>`);
            para = [];
        };

        let i = 0;
        while (i < lines.length) {
            const line = lines[i];

            if (!line.trim()) {
                flushParagraph();
                i++;
                continue;
            }

            // Code-Block mit ``` oder ~~~
            let m = line.match(FENCE_RE);
            if (m && !(m[2][0] === '`' && m[3].includes('`'))) {
                flushParagraph();
                const fence = m[2];
                const indent = m[1].length;
                const closeRe = new RegExp(`^ {0,3}${fence[0] === '`' ? '`' : '~'}{${fence.length},}[ \\t]*$`);
                const code = [];
                i++;
                while (i < lines.length && !closeRe.test(lines[i])) {
                    code.push(indent ? lines[i].replace(new RegExp(`^ {0,${indent}}`), '') : lines[i]);
                    i++;
                }
                i++; // schließende Fence
                out.push(`<pre><code>${esc(code.join('\n'))}</code></pre>`);
                continue;
            }

            // # Überschrift
            m = line.match(ATX_HEADING_RE);
            if (m) {
                flushParagraph();
                const level = m[1].length;
                out.push(`<h${level}>${parseInline((m[2] || '').trim(), ctx)}</h${level}>`);
                i++;
                continue;
            }

            // Setext-Überschrift (Text, darunter === oder ---)
            if (para.length && /^ {0,3}(=+|-{2,})[ \t]*$/.test(line)) {
                const level = line.trim()[0] === '=' ? 1 : 2;
                const inner = parseInline(para.join('\n').trim(), ctx);
                para = [];
                out.push(`<h${level}>${inner}</h${level}>`);
                i++;
                continue;
            }

            // Trennlinie
            if (THEMATIC_BREAK_RE.test(line)) {
                flushParagraph();
                out.push('<hr>');
                i++;
                continue;
            }

            // > Zitat
            if (/^ {0,3}>/.test(line)) {
                flushParagraph();
                const quote = [];
                while (i < lines.length) {
                    const l = lines[i];
                    if (/^ {0,3}>/.test(l)) {
                        quote.push(l.replace(/^ {0,3}> ?/, ''));
                        i++;
                    } else if (l.trim() && quote.length && quote[quote.length - 1].trim() && !startsBlock(l)) {
                        quote.push(l); // "lazy" Fortsetzung des Absatzes
                        i++;
                    } else {
                        break;
                    }
                }
                out.push(`<blockquote>${parseBlocks(quote, ctx, false)}</blockquote>`);
                continue;
            }

            // Listen (leere Einträge / Nummern ≠ 1 unterbrechen keinen Absatz)
            m = line.match(LIST_ITEM_RE);
            if (m && !(para.length && (!m[4].trim() || (/\d/.test(m[2]) && parseInt(m[2], 10) !== 1)))) {
                flushParagraph();
                const list = parseList(lines, i, ctx);
                out.push(list.html);
                i = list.next;
                continue;
            }

            // GFM-Tabelle
            if (line.includes('|') && i + 1 < lines.length &&
                lines[i + 1].includes('|') && TABLE_DELIMITER_RE.test(lines[i + 1])) {
                const header = splitTableRow(line);
                const aligns = splitTableRow(lines[i + 1]).map(cell => {
                    const left = cell.startsWith(':');
                    const right = cell.endsWith(':');
                    return left && right ? 'center' : right ? 'right' : left ? 'left' : '';
                });
                if (header.length === aligns.length) {
                    flushParagraph();
                    i += 2;
                    const rows = [];
                    while (i < lines.length && lines[i].trim() && lines[i].includes('|')) {
                        rows.push(splitTableRow(lines[i]));
                        i++;
                    }
                    const cell = (tag, text, idx) =>
                        `<${tag}${aligns[idx] ? ` align="${aligns[idx]}"` : ''}>${parseInline(text || '', ctx)}</${tag}>`;
                    let table = `<table><thead><tr>${header.map((c, idx) => cell('th', c, idx)).join('')}</tr></thead>`;
                    if (rows.length) {
                        table += `<tbody>${rows.map(r =>
                            `<tr>${aligns.map((_, idx) => cell('td', r[idx], idx)).join('')}</tr>`
                        ).join('')}</tbody>`;
                    }
                    out.push(table + '</table>');
                    continue;
                }
            }

            // HTML-Block: wird unverändert übernommen (Bereinigung erfolgt später)
            const htmlType = htmlBlockType(line, para.length > 0);
            if (htmlType) {
                flushParagraph();
                const block = [line];
                i++;
                if (htmlType.endRe) {
                    if (!htmlType.endRe.test(line)) {
                        while (i < lines.length) {
                            block.push(lines[i]);
                            i++;
                            if (htmlType.endRe.test(lines[i - 1])) break;
                        }
                    }
                } else {
                    while (i < lines.length && lines[i].trim()) {
                        block.push(lines[i]);
                        i++;
                    }
                }
                out.push(block.join('\n'));
                continue;
            }

            para.push(line.replace(/^[ \t]+/, ''));
            i++;
        }

        flushParagraph();
        return out.join('\n');
    }

    function parseList(lines, start, ctx) {
        const first = lines[start].match(LIST_ITEM_RE);
        const ordered = /\d/.test(first[2]);
        const sameKind = mm => /\d/.test(mm[2]) === ordered;
        const items = [];
        let current = null;
        let loose = false;
        let pendingBlank = false;
        let i = start;

        while (i < lines.length) {
            const line = lines[i];

            if (!line.trim()) {
                if (!current) break;
                let j = i + 1;
                while (j < lines.length && !lines[j].trim()) j++;
                if (j >= lines.length) break;
                const nextMatch = lines[j].match(LIST_ITEM_RE);
                const continues = indentOf(lines[j]) >= current.contentIndent ||
                    (nextMatch && sameKind(nextMatch) && !THEMATIC_BREAK_RE.test(lines[j]));
                if (!continues) break;
                current.lines.push('');
                pendingBlank = true;
                i++;
                continue;
            }

            const indent = indentOf(line);
            const mm = line.match(LIST_ITEM_RE);

            if (mm && !THEMATIC_BREAK_RE.test(line) && (!current || indent < current.contentIndent)) {
                if (!sameKind(mm)) break;
                if (current) items.push(current);
                if (pendingBlank && current) loose = true;
                const spaces = mm[3].length;
                const contentIndent = indent + mm[2].length + (spaces === 0 || spaces > 4 ? 1 : spaces);
                current = {
                    lines: [mm[4]],
                    contentIndent,
                    number: ordered ? parseInt(mm[2], 10) : null,
                    innerBlank: false
                };
                pendingBlank = false;
                i++;
                continue;
            }

            // Eine Liste beginnt immer mit einem Eintrag – ohne ihn gibt es nichts fortzusetzen
            if (!current) break;

            if (indent >= current.contentIndent) {
                const content = line.slice(current.contentIndent);
                if (pendingBlank && !isListItem(content)) current.innerBlank = true;
                current.lines.push(content);
                pendingBlank = false;
                i++;
                continue;
            }

            // "lazy" Fortsetzungszeile des Absatzes im Listeneintrag
            if (!pendingBlank && !startsBlock(line)) {
                current.lines.push(line.trim());
                i++;
                continue;
            }
            break;
        }
        if (current) items.push(current);
        if (items.some(item => item.innerBlank)) loose = true;

        const tag = ordered ? 'ol' : 'ul';
        const startAttr = ordered && items[0].number !== 1 ? ` start="${items[0].number}"` : '';
        const body = items.map(item => {
            let content = parseBlocks(item.lines, ctx, !loose);
            // GFM-Aufgabenliste: - [ ] / - [x]
            content = content.replace(/^(<p>)?\[([ xX])][ \t]+/, (_, p, mark) =>
                (p || '') + (mark === ' ' ? '☐ ' : '☑ '));
            return `<li>${content}</li>`;
        }).join('');

        return { html: `<${tag}${startAttr}>${body}</${tag}>`, next: i };
    }

    function splitTableRow(row) {
        let s = row.trim();
        if (s.startsWith('|')) s = s.slice(1);
        if (s.endsWith('|') && !s.endsWith('\\|')) s = s.slice(0, -1);
        const cells = [];
        let cur = '';
        let inCode = false;
        for (let k = 0; k < s.length; k++) {
            const c = s[k];
            if (c === '\\' && s[k + 1] === '|') {
                cur += '|';
                k++;
                continue;
            }
            if (c === '`') inCode = !inCode;
            if (c === '|' && !inCode) {
                cells.push(cur.trim());
                cur = '';
                continue;
            }
            cur += c;
        }
        cells.push(cur.trim());
        return cells;
    }

    // ── Inline-Ebene ───────────────────────────────────────────────────────────
    function matchHtmlAt(text, pos) {
        const rest = text.slice(pos);
        const m = rest.match(HTML_COMMENT) || rest.match(HTML_OPEN_TAG) || rest.match(HTML_CLOSE_TAG);
        return m ? m[0] : null;
    }

    function skipCodeSpan(text, pos) {
        let run = 0;
        while (text[pos + run] === '`') run++;
        const ticks = '`'.repeat(run);
        let search = pos + run;
        while (true) {
            const close = text.indexOf(ticks, search);
            if (close === -1) return { found: false, end: pos + run };
            let after = close + run;
            if (text[after] === '`') {
                // längerer Backtick-Lauf → kein passender Abschluss
                while (text[after] === '`') after++;
                search = after;
                continue;
            }
            return { found: true, contentStart: pos + run, contentEnd: close, end: after };
        }
    }

    function findClosingBracket(text, open) {
        let depth = 0;
        for (let k = open; k < text.length; k++) {
            const c = text[k];
            if (c === '\\') {
                k++;
                continue;
            }
            if (c === '`') {
                k = skipCodeSpan(text, k).end - 1;
                continue;
            }
            if (c === '<') {
                const html = matchHtmlAt(text, k);
                if (html) {
                    k += html.length - 1;
                    continue;
                }
            }
            if (c === '[') depth++;
            else if (c === ']') {
                depth--;
                if (depth === 0) return k;
            }
        }
        return -1;
    }

    /** Parst `(ziel "titel")` ab der öffnenden Klammer. */
    function parseLinkDestination(text, pos) {
        let k = pos + 1;
        while (k < text.length && /[ \t\n]/.test(text[k])) k++;
        let dest = '';
        if (text[k] === '<') {
            const close = text.indexOf('>', k);
            if (close === -1 || text.slice(k, close).includes('\n')) return null;
            dest = text.slice(k + 1, close);
            k = close + 1;
        } else {
            let depth = 0;
            while (k < text.length) {
                const c = text[k];
                if (c === '\\' && k + 1 < text.length && /[!-/:-@[-`{-~]/.test(text[k + 1])) {
                    dest += text[k + 1];
                    k += 2;
                    continue;
                }
                if (/\s/.test(c)) break;
                if (c === '(') depth++;
                if (c === ')') {
                    if (depth === 0) break;
                    depth--;
                }
                dest += c;
                k++;
            }
        }
        while (k < text.length && /[ \t\n]/.test(text[k])) k++;
        let title = '';
        const quote = text[k];
        if (quote === '"' || quote === "'" || (quote === '(' && dest)) {
            const closeChar = quote === '(' ? ')' : quote;
            const close = text.indexOf(closeChar, k + 1);
            if (close === -1) return null;
            title = text.slice(k + 1, close);
            k = close + 1;
            while (k < text.length && /[ \t\n]/.test(text[k])) k++;
        }
        if (text[k] !== ')') return null;
        return { dest, title, end: k + 1 };
    }

    /** Link `[text](url)` / Bild `![alt](url)` inkl. Referenz-Varianten. `open` zeigt auf `[`. */
    function parseLinkOrImage(text, open, ctx, isImage) {
        const close = findClosingBracket(text, open);
        if (close === -1) return null;
        const label = text.slice(open + 1, close);
        let target = null;
        let end = close + 1;

        if (text[close + 1] === '(') {
            const parsed = parseLinkDestination(text, close + 1);
            if (parsed) {
                target = { href: parsed.dest, title: parsed.title };
                end = parsed.end;
            }
        }
        if (!target && text[close + 1] === '[') {
            const refClose = text.indexOf(']', close + 2);
            if (refClose !== -1) {
                const refName = (text.slice(close + 2, refClose) || label).trim().toLowerCase();
                if (ctx.refs.has(refName)) {
                    target = ctx.refs.get(refName);
                    end = refClose + 1;
                }
            }
        }
        if (!target) {
            const refName = label.trim().toLowerCase();
            if (ctx.refs.has(refName)) target = ctx.refs.get(refName);
        }
        if (!target) return null;

        const titleAttr = target.title ? ` title="${esc(target.title)}"` : '';
        if (isImage) {
            const alt = parseInline(label, ctx).replace(/<[^>]*>/g, '');
            return { html: `<img src="${esc(target.href)}" alt="${alt}"${titleAttr}>`, end };
        }
        // Links in Links sind nicht erlaubt → Inhalt ohne weitere Links rendern
        const inner = parseInline(label, Object.assign({}, ctx, { inLink: true }));
        return { html: `<a href="${esc(target.href)}"${titleAttr}>${inner}</a>`, end };
    }

    function findEmphasisCloser(text, from, ch, size) {
        const isWordChar = c => !!c && /[\p{L}\p{N}]/u.test(c);
        let fallback = -1;
        let k = from;
        while (k < text.length) {
            const c = text[k];
            if (c === '\\') {
                k += 2;
                continue;
            }
            if (c === '`') {
                k = skipCodeSpan(text, k).end;
                continue;
            }
            if (c === '<') {
                const html = matchHtmlAt(text, k);
                if (html) {
                    k += html.length;
                    continue;
                }
            }
            if (c === '[') {
                const close = findClosingBracket(text, k);
                if (close !== -1) {
                    const dest = text[close + 1] === '(' ? parseLinkDestination(text, close + 1) : null;
                    k = dest ? dest.end : close + 1;
                    continue;
                }
            }
            if (c === ch) {
                let len = 1;
                while (text[k + len] === ch) len++;
                const before = text[k - 1];
                const after = text[k + len];
                const rightFlanking = k > from && !/\s/.test(before);
                const underscoreOk = ch !== '_' || !isWordChar(after);
                if (rightFlanking && underscoreOk) {
                    if (len === size) return k;
                    if (len > size && fallback === -1) fallback = k + len - size;
                }
                k += len;
                continue;
            }
            k++;
        }
        return fallback;
    }

    function parseEmphasis(text, i, ctx) {
        const ch = text[i];
        let run = 1;
        while (text[i + run] === ch) run++;
        const next = text[i + run];
        const prev = i > 0 ? text[i - 1] : '';
        if (!next || /\s/.test(next)) return null;
        if (ch === '_' && prev && /[\p{L}\p{N}]/u.test(prev)) return null;

        if (ch === '~') {
            if (run !== 2) return null;
            const close = findEmphasisCloser(text, i + 2, '~', 2);
            if (close === -1) return null;
            return { html: `<del>${parseInline(text.slice(i + 2, close), ctx)}</del>`, end: close + 2 };
        }

        for (let size = Math.min(run, 3); size >= 1; size--) {
            const contentStart = i + run;
            const close = findEmphasisCloser(text, contentStart, ch, size);
            if (close === -1 || close <= contentStart) continue;
            const inner = parseInline(text.slice(contentStart, close), ctx);
            const wrapped = size === 3
                ? `<strong><em>${inner}</em></strong>`
                : size === 2 ? `<strong>${inner}</strong>` : `<em>${inner}</em>`;
            return { html: esc(ch.repeat(run - size)) + wrapped, end: close + size };
        }
        return null;
    }

    const BARE_URL_RE = /^(?:https?:\/\/|www\.)[^\s<]*[^\s<?!.,:;*_~)'"]/i;

    function parseInline(text, ctx) {
        let out = '';
        let i = 0;
        const n = text.length;

        while (i < n) {
            const c = text[i];

            if (c === '\\' && i + 1 < n) {
                const next = text[i + 1];
                if (next === '\n') {
                    out += '<br>\n';
                    i += 2;
                    continue;
                }
                if (/[!-/:-@[-`{-~]/.test(next)) {
                    out += esc(next);
                    i += 2;
                    continue;
                }
            }

            if (c === '`') {
                const span = skipCodeSpan(text, i);
                if (span.found) {
                    let code = text.slice(span.contentStart, span.contentEnd).replace(/\n/g, ' ');
                    if (/^ .*[^ ].* $/.test(code)) code = code.slice(1, -1);
                    out += `<code>${esc(code)}</code>`;
                } else {
                    out += text.slice(i, span.end);
                }
                i = span.end;
                continue;
            }

            if (c === '<') {
                const rest = text.slice(i, i + 2048);
                const auto = rest.match(/^<((?:https?:\/\/|mailto:)[^\s<>]+)>/i);
                if (auto) {
                    out += ctx.inLink ? esc(auto[1]) : `<a href="${esc(auto[1])}">${esc(auto[1])}</a>`;
                    i += auto[0].length;
                    continue;
                }
                const html = matchHtmlAt(text, i);
                if (html) {
                    out += html;
                    i += html.length;
                    continue;
                }
                out += '&lt;';
                i++;
                continue;
            }

            if (c === '!' && text[i + 1] === '[') {
                const image = parseLinkOrImage(text, i + 1, ctx, true);
                if (image) {
                    out += image.html;
                    i = image.end;
                    continue;
                }
            }

            if (c === '[' && !ctx.inLink) {
                const link = parseLinkOrImage(text, i, ctx, false);
                if (link) {
                    out += link.html;
                    i = link.end;
                    continue;
                }
            }

            if (c === '*' || c === '_' || c === '~') {
                const emphasis = parseEmphasis(text, i, ctx);
                if (emphasis) {
                    out += emphasis.html;
                    i = emphasis.end;
                    continue;
                }
                let run = 1;
                while (text[i + run] === c) run++;
                out += c.repeat(run);
                i += run;
                continue;
            }

            if ((c === 'h' || c === 'w' || c === 'H' || c === 'W') && !ctx.inLink &&
                (i === 0 || /[\s(*_~]/.test(text[i - 1]))) {
                const lower = text.slice(i, i + 8).toLowerCase();
                if (lower.startsWith('http://') || lower.startsWith('https://') || lower.startsWith('www.')) {
                    const m = text.slice(i, i + 2048).match(BARE_URL_RE);
                    if (m) {
                        const href = /^www\./i.test(m[0]) ? 'https://' + m[0] : m[0];
                        out += `<a href="${esc(href)}">${esc(m[0])}</a>`;
                        i += m[0].length;
                        continue;
                    }
                }
            }

            if (c === ' ' && text[i + 1] === ' ') {
                const m = text.slice(i).match(/^ {2,}\n/);
                if (m) {
                    out += '<br>\n';
                    i += m[0].length;
                    continue;
                }
            }

            if (c === '&') {
                const m = text.slice(i, i + 40).match(/^&(?:#\d{1,7}|#[xX][0-9a-fA-F]{1,6}|[a-zA-Z][a-zA-Z0-9]{1,31});/);
                if (m) {
                    out += m[0];
                    i += m[0].length;
                    continue;
                }
                out += '&amp;';
                i++;
                continue;
            }

            out += c === '>' ? '&gt;' : c === '"' ? '&quot;' : c;
            i++;
        }
        return out;
    }

    function markdownToHtml(source) {
        const text = String(source || '').replace(/\r\n?/g, '\n').replace(/\t/g, '    ');
        const refs = new Map();
        const lines = [];
        let fence = null;
        for (const line of text.split('\n')) {
            const fenceMatch = line.match(FENCE_RE);
            if (fenceMatch) {
                if (!fence) fence = fenceMatch[2][0];
                else if (fenceMatch[2][0] === fence && !fenceMatch[3].trim()) fence = null;
                lines.push(line);
                continue;
            }
            if (!fence) {
                const def = line.match(REF_DEF_RE);
                if (def) {
                    const name = def[1].trim().toLowerCase();
                    if (!refs.has(name)) refs.set(name, { href: def[2], title: def[3] || def[4] || def[5] || '' });
                    continue;
                }
            }
            lines.push(line);
        }
        return parseBlocks(lines, { refs, inLink: false }, false);
    }

    // ── Bereinigung (Whitelist) ────────────────────────────────────────────────
    const ALLOWED_TAGS = new Set([
        'a', 'abbr', 'b', 'bdi', 'bdo', 'blockquote', 'br', 'caption', 'center', 'cite', 'code',
        'col', 'colgroup', 'dd', 'del', 'details', 'dfn', 'div', 'dl', 'dt', 'em', 'figcaption',
        'figure', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'hr', 'i', 'img', 'ins', 'kbd', 'li', 'mark',
        'ol', 'p', 'pre', 'q', 'rp', 'rt', 'ruby', 's', 'samp', 'small', 'span', 'strike', 'strong',
        'sub', 'summary', 'sup', 'table', 'tbody', 'td', 'tfoot', 'th', 'thead', 'time', 'tr', 'tt',
        'u', 'ul', 'var', 'wbr'
    ]);
    // Werden samt Inhalt entfernt
    const DROPPED_TAGS = new Set([
        'script', 'style', 'noscript', 'template', 'object', 'embed', 'applet', 'form', 'input',
        'button', 'select', 'option', 'optgroup', 'textarea', 'svg', 'math', 'link', 'meta', 'base',
        'head', 'title', 'frame', 'frameset', 'canvas', 'dialog', 'source', 'track', 'portal'
    ]);
    const GLOBAL_ATTRS = new Set(['title', 'align', 'lang', 'dir']);
    const TAG_ATTRS = {
        a: ['href'],
        img: ['src', 'alt', 'width', 'height'],
        td: ['colspan', 'rowspan', 'valign'],
        th: ['colspan', 'rowspan', 'valign', 'scope'],
        ol: ['start', 'type', 'reversed'],
        li: ['value'],
        details: ['open'],
        col: ['span'],
        colgroup: ['span'],
        time: ['datetime']
    };
    const URL_BASE = 'https://modrinth.com/';

    function safeUrl(value, protocols) {
        if (!value) return null;
        try {
            const url = new URL(String(value).trim(), URL_BASE);
            return protocols.includes(url.protocol) ? url.href : null;
        } catch (_e) {
            return null;
        }
    }

    function safeImageUrl(value) {
        const raw = String(value || '').trim();
        if (/^data:image\/(png|gif|jpe?g|webp|avif);base64,/i.test(raw)) return raw;
        return safeUrl(raw, ['https:', 'http:']);
    }

    function youtubeWatchUrl(src) {
        const url = safeUrl(src, ['https:', 'http:']);
        if (!url) return null;
        const m = url.match(/^https?:\/\/(?:www\.)?(?:youtube(?:-nocookie)?\.com\/embed\/|youtu\.be\/)([\w-]{6,})/i);
        return m ? `https://www.youtube.com/watch?v=${m[1]}` : null;
    }

    function mediaLink(doc, href, label) {
        const p = doc.createElement('p');
        const a = doc.createElement('a');
        a.setAttribute('href', href);
        a.setAttribute('target', '_blank');
        a.setAttribute('rel', 'noopener noreferrer');
        a.textContent = label;
        p.appendChild(a);
        return p;
    }

    function sanitizeChildren(parent, doc) {
        for (const node of Array.from(parent.childNodes)) {
            if (node.nodeType === 3) continue; // Text
            if (node.nodeType !== 1) {
                node.remove(); // Kommentare etc.
                continue;
            }
            const tag = node.tagName.toLowerCase();

            if (tag === 'iframe') {
                const watch = youtubeWatchUrl(node.getAttribute('src'));
                if (watch) node.replaceWith(mediaLink(doc, watch, '▶ Video auf YouTube ansehen'));
                else node.remove();
                continue;
            }
            if (tag === 'video' || tag === 'audio') {
                const src = node.getAttribute('src') || node.querySelector('source[src]')?.getAttribute('src');
                const href = safeUrl(src, ['https:', 'http:']);
                if (href) node.replaceWith(mediaLink(doc, href, tag === 'video' ? '▶ Video ansehen' : '▶ Audio anhören'));
                else node.remove();
                continue;
            }
            if (DROPPED_TAGS.has(tag)) {
                node.remove();
                continue;
            }
            if (!ALLOWED_TAGS.has(tag)) {
                // Unbekannte Tags (z.B. <font>, <picture>) auflösen, Inhalt behalten
                sanitizeChildren(node, doc);
                node.replaceWith(...Array.from(node.childNodes));
                continue;
            }

            const allowed = TAG_ATTRS[tag] || [];
            for (const attr of Array.from(node.attributes)) {
                const name = attr.name.toLowerCase();
                if (!GLOBAL_ATTRS.has(name) && !allowed.includes(name)) node.removeAttribute(attr.name);
            }
            for (const dim of ['width', 'height']) {
                const v = node.getAttribute(dim);
                if (v != null && !/^\d{1,5}(\.\d+)?(%|px)?$/.test(v.trim())) node.removeAttribute(dim);
            }

            if (tag === 'a') {
                const href = safeUrl(node.getAttribute('href'), ['https:', 'http:', 'mailto:']);
                if (href) {
                    node.setAttribute('href', href);
                    node.setAttribute('target', '_blank');
                    node.setAttribute('rel', 'noopener noreferrer');
                } else {
                    node.removeAttribute('href');
                }
            } else if (tag === 'img') {
                const src = safeImageUrl(node.getAttribute('src'));
                if (!src) {
                    node.remove();
                    continue;
                }
                node.setAttribute('src', src);
                node.setAttribute('loading', 'lazy');
                node.setAttribute('decoding', 'async');
                // Manche Bild-Hoster blockieren fremde Referrer (Hotlink-Schutz)
                node.setAttribute('referrerpolicy', 'no-referrer');
            }

            sanitizeChildren(node, doc);
        }
    }

    function sanitizeHtml(html) {
        const doc = new DOMParser().parseFromString(`<!DOCTYPE html><body><div id="lion-md-root">${html}</div></body>`, 'text/html');
        const root = doc.getElementById('lion-md-root');
        if (!root) return '';
        sanitizeChildren(root, doc);
        return root.innerHTML;
    }

    function renderMarkdown(source) {
        return sanitizeHtml(markdownToHtml(source));
    }

    const api = { markdownToHtml, sanitizeHtml, renderMarkdown };
    if (typeof module !== 'undefined' && module.exports) module.exports = api;
    global.LionMarkdown = api;
})(typeof window !== 'undefined' ? window : globalThis);

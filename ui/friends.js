// Freunde & geteilte Instanzen (angelehnt an "Shared Instances" der Modrinth App).
//
// Die Launcher verbinden sich direkt per P2P (iroh, Ende-zu-Ende-verschlüsselt) – es gibt
// keinen eigenen Server. Freunde werden per Freundescode hinzugefügt, Instanzen per
// Einladung oder Einladungslink geteilt. Updates werden beim Spielstart geprüft und erst
// nach Bestätigung übernommen.
//
// Nutzt Hilfsfunktionen aus app.js (invoke, showToast, escapeHtml, escapeAttr, showConfirmModal,
// closeDynamicModal, profiles, prepareProfileDownloadFlow, refreshProfilesInBackground, ...).

const friendsUi = {
    state: null,
    ready: false,
    refreshTimer: null
};

/** Vom Besitzer verwaltete Dateien pro Profil (für die Sperre im Content-Tab). */
const sharedManagedFiles = new Map();

const SHARE_KIND_LABELS = { mods: 'Mod', resourcepacks: 'Resource Pack', shaderpacks: 'Shader Pack' };

function playerHeadUrl(uuid, name, size = 32) {
    const key = uuid || name || 'MHF_Steve';
    return `https://mc-heads.net/avatar/${encodeURIComponent(key)}/${size}`;
}

function copyToClipboard(text, successMessage) {
    navigator.clipboard.writeText(text)
        .then(() => showToast(successMessage, 'success', 2200))
        .catch(() => showToast('Kopieren fehlgeschlagen', 'error', 2500));
}

// ==================== Freunde-Kasten ====================

function initFriends() {
    document.querySelectorAll('[data-friends-panel]').forEach(buildFriendsPanelSkeleton);
    refreshFriends();

    try {
        window.__TAURI__.event.listen('friends-changed', () => scheduleFriendsRefresh());
    } catch (_e) {
        // Event-API nicht verfügbar – Polling reicht
    }
    // Online-Status läuft ab, auch ohne Event → regelmäßig neu zeichnen
    setInterval(refreshFriends, 15000);
}

function scheduleFriendsRefresh() {
    clearTimeout(friendsUi.refreshTimer);
    friendsUi.refreshTimer = setTimeout(refreshFriends, 250);
}

async function refreshFriends() {
    try {
        friendsUi.state = await invoke('friends_state');
        friendsUi.ready = true;
    } catch (_e) {
        // Dienst startet noch
        friendsUi.ready = false;
    }
    renderFriendsPanels();
    refreshOpenShareTab();
}

function buildFriendsPanelSkeleton(panel) {
    panel.innerHTML = `
        <div class="friends-header">
            <span class="friends-title"><i class="bi bi-people-fill"></i> Freunde</span>
            <span class="friends-online-count" data-friends-count></span>
            <button class="friends-icon-btn" data-friends-toggle-add title="Freund hinzufügen" aria-label="Freund hinzufügen">
                <i class="bi bi-person-plus-fill"></i>
            </button>
        </div>
        <form class="friends-add hidden" data-friends-add-form>
            <input type="text" data-friends-add-input aria-label="Freundescode oder Einladungslink"
                   placeholder="Freundescode oder Einladungslink" autocomplete="off" spellcheck="false">
            <button type="submit" class="btn" title="Hinzufügen"><i class="bi bi-plus-lg"></i></button>
        </form>
        <div class="friends-list" data-friends-list></div>
        <div class="friends-me" data-friends-me></div>
    `;

    const form = panel.querySelector('[data-friends-add-form]');
    const input = panel.querySelector('[data-friends-add-input]');
    panel.querySelector('[data-friends-toggle-add]').addEventListener('click', () => {
        form.classList.toggle('hidden');
        if (!form.classList.contains('hidden')) input.focus();
    });
    form.addEventListener('submit', async (event) => {
        event.preventDefault();
        const value = input.value.trim();
        if (!value) return;
        if (await addFriendOrJoin(value)) {
            input.value = '';
            form.classList.add('hidden');
        }
    });
}

/** Freundescode → Anfrage senden; Einladungslink → Vorschau der geteilten Instanz. */
async function addFriendOrJoin(value) {
    if (value.startsWith('lion://share/')) {
        openSharePreview({ link: value });
        return true;
    }
    try {
        const message = await invoke('friends_add', { code: value });
        showToast(message, 'success', 3500);
        refreshFriends();
        return true;
    } catch (error) {
        showToast(String(error), 'error', 4000);
        return false;
    }
}

function renderFriendsPanels() {
    document.querySelectorAll('[data-friends-panel]').forEach(panel => {
        const list = panel.querySelector('[data-friends-list]');
        const me = panel.querySelector('[data-friends-me]');
        const count = panel.querySelector('[data-friends-count]');
        if (!list || !me) return;

        if (!friendsUi.ready || !friendsUi.state) {
            count.textContent = '';
            list.innerHTML = `<div class="friends-empty"><span class="lion-spin"><i class="bi bi-arrow-repeat"></i></span> Verbinde…</div>`;
            me.innerHTML = '';
            return;
        }

        const { friends, invites } = friendsUi.state;
        const accepted = friends.filter(f => f.state === 'accepted');
        const online = accepted.filter(f => f.online);
        const offline = accepted.filter(f => !f.online);
        const incoming = friends.filter(f => f.state === 'incoming');
        const outgoing = friends.filter(f => f.state === 'outgoing');

        count.textContent = accepted.length ? `${online.length} online` : '';

        let html = '';
        if (incoming.length || outgoing.length) {
            html += friendsSection('Anfragen', [
                ...incoming.map(f => friendRow(f, 'incoming')),
                ...outgoing.map(f => friendRow(f, 'outgoing'))
            ]);
        }
        if (invites.length) {
            html += friendsSection('Einladungen', invites.map(inviteRow));
        }
        html += friendsSection(`Online — ${online.length}`, online.map(f => friendRow(f, 'accepted')));
        html += friendsSection(`Offline — ${offline.length}`, offline.map(f => friendRow(f, 'accepted')));
        if (!accepted.length && !incoming.length && !outgoing.length) {
            html += `<div class="friends-empty">Noch keine Freunde. Teile deinen Freundescode unten oder füge einen über <i class="bi bi-person-plus-fill"></i> hinzu.</div>`;
        }
        list.innerHTML = html;

        const self = friendsUi.state.me;
        me.innerHTML = `
            <img class="friends-avatar" alt="" src="${escapeAttr(playerHeadUrl(self.uuid, self.name))}">
            <div class="friends-me-text">
                <div class="friends-name">${escapeHtml(self.name)}</div>
                <div class="friends-sub"><span class="friends-dot online"></span> Online</div>
            </div>
            <button class="friends-icon-btn" data-copy-code title="Freundescode kopieren" aria-label="Freundescode kopieren">
                <i class="bi bi-clipboard"></i>
            </button>
        `;
        me.querySelector('[data-copy-code]').addEventListener('click',
            () => copyToClipboard(self.code, 'Freundescode kopiert – schick ihn deinem Freund'));
    });
}

function friendsSection(title, rows) {
    if (!rows.length) return '';
    return `<div class="friends-section"><div class="friends-section-title">${escapeHtml(title)}</div>${rows.join('')}</div>`;
}

function friendRow(friend, kind) {
    const id = escapeAttr(friend.id);
    const offline = kind === 'accepted' && !friend.online;
    let sub = friend.online ? 'Online' : 'Offline';
    let actions = `<button class="friends-icon-btn friends-row-action" onclick="removeFriend('${id}')" title="Freund entfernen" aria-label="Freund entfernen"><i class="bi bi-x-lg"></i></button>`;
    if (kind === 'incoming') {
        sub = 'Möchte dein Freund sein';
        actions = `
            <button class="friends-icon-btn accept" onclick="acceptFriend('${id}')" title="Annehmen" aria-label="Annehmen"><i class="bi bi-check-lg"></i></button>
            <button class="friends-icon-btn" onclick="removeFriend('${id}', true)" title="Ablehnen" aria-label="Ablehnen"><i class="bi bi-x-lg"></i></button>`;
    } else if (kind === 'outgoing') {
        sub = 'Anfrage gesendet';
        actions = `<button class="friends-icon-btn" onclick="removeFriend('${id}', true)" title="Anfrage zurückziehen" aria-label="Anfrage zurückziehen"><i class="bi bi-x-lg"></i></button>`;
    }
    const stateClass = offline ? ' is-offline' : (kind === 'accepted' ? '' : ' is-pending');
    return `
        <div class="friends-row${stateClass}" title="${escapeAttr(friend.name)}">
            <div class="friends-avatar-wrap">
                <img class="friends-avatar" alt="" src="${escapeAttr(playerHeadUrl(friend.uuid, friend.name))}">
                ${kind === 'accepted' ? `<span class="friends-dot ${friend.online ? 'online' : ''}"></span>` : ''}
            </div>
            <div class="friends-row-text">
                <div class="friends-name">${escapeHtml(friend.name)}</div>
                <div class="friends-sub">${escapeHtml(sub)}</div>
            </div>
            <div class="friends-row-actions">${actions}</div>
        </div>`;
}

function inviteRow(invite) {
    const owner = escapeAttr(invite.owner_id);
    const share = escapeAttr(invite.share_id);
    const icon = invite.icon
        ? `<img class="friends-avatar square" alt="" src="${escapeAttr(invite.icon)}" data-fallback="logo">`
        : `<img class="friends-avatar square" alt="" src="${escapeAttr(LION_LOGO_URL)}">`;
    return `
        <div class="friends-row invite" onclick="openSharePreview({ ownerId: '${owner}', shareId: '${share}' })" title="Einladung ansehen">
            <div class="friends-avatar-wrap">${icon}</div>
            <div class="friends-row-text">
                <div class="friends-name">${escapeHtml(invite.name)}</div>
                <div class="friends-sub">von ${escapeHtml(invite.owner_name)}</div>
            </div>
            <div class="friends-row-actions">
                <button class="friends-icon-btn" onclick="event.stopPropagation(); declineShareInvite('${owner}', '${share}')" title="Ablehnen" aria-label="Ablehnen"><i class="bi bi-x-lg"></i></button>
            </div>
        </div>`;
}

async function acceptFriend(id) {
    try {
        await invoke('friends_accept', { id });
        showToast('Freundschaftsanfrage angenommen', 'success', 2500);
    } catch (error) {
        showToast(String(error), 'error', 4000);
    }
    refreshFriends();
}

async function removeFriend(id, isRequest = false) {
    const friend = friendsUi.state?.friends.find(f => f.id === id);
    if (!isRequest) {
        const confirmed = await showConfirmModal({
            title: 'Freund entfernen',
            message: 'Möchtest du ',
            highlight: friend?.name || 'diesen Freund',
            suffix: ' wirklich entfernen?',
            confirmText: 'Entfernen',
            icon: 'bi-person-dash'
        });
        if (!confirmed) return;
    }
    try {
        await invoke('friends_remove', { id });
    } catch (error) {
        showToast(String(error), 'error', 4000);
    }
    refreshFriends();
}

async function declineShareInvite(ownerId, shareId) {
    try {
        await invoke('share_decline_invite', { ownerId, shareId });
    } catch (error) {
        showToast(String(error), 'error', 4000);
    }
    refreshFriends();
}

/** Freunde-Kasten in der Profil-Detailansicht ausblenden (dort wird der Platz gebraucht). */
function setFriendsPanelHidden(hidden) {
    document.getElementById('page-profiles')?.classList.toggle('friends-collapsed', hidden);
}

// ==================== Einladung ansehen & annehmen ====================

function shareModalShell(id, innerHtml) {
    closeDynamicModal(id);
    const modal = document.createElement('div');
    modal.id = id;
    modal.className = 'share-modal-backdrop';
    modal.innerHTML = `<div class="share-modal" onclick="event.stopPropagation()">${innerHtml}</div>`;
    modal.addEventListener('click', () => closeDynamicModal(id));
    document.body.appendChild(modal);
    return modal;
}

function shareItemRow(item, extraClass = '') {
    const version = item.version || item.new_version || '';
    return `
        <div class="share-item ${extraClass}">
            <span class="share-item-kind">${escapeHtml(SHARE_KIND_LABELS[item.kind] || item.kind)}</span>
            <span class="share-item-name">${escapeHtml(item.name)}</span>
            ${item.old_version && item.new_version ? `<span class="share-item-version">${escapeHtml(item.old_version)} → ${escapeHtml(item.new_version)}</span>`
                : version ? `<span class="share-item-version">${escapeHtml(version)}</span>` : ''}
            ${item.unpublished ? '<span class="share-badge warn" title="Nicht auf Modrinth veröffentlicht – kommt direkt vom Besitzer">Nicht auf Modrinth</span>' : ''}
        </div>`;
}

/** params: { link } oder { ownerId, shareId } */
async function openSharePreview(params) {
    const modal = shareModalShell('share-preview-modal', `
        <div class="share-modal-loading"><span class="lion-spin"><i class="bi bi-arrow-repeat"></i></span> Lade Einladung…</div>`);
    let preview;
    try {
        preview = await invoke('share_preview', {
            link: params.link || null,
            ownerId: params.ownerId || null,
            shareId: params.shareId || null
        });
    } catch (error) {
        modal.remove();
        showToast('Einladung konnte nicht geladen werden: ' + error, 'error', 5000);
        return;
    }

    const items = preview.items.slice().sort((a, b) => a.kind.localeCompare(b.kind) || a.name.localeCompare(b.name));
    const icon = preview.icon || LION_LOGO_URL;
    modal.querySelector('.share-modal').innerHTML = `
        <div class="share-modal-head">
            <img class="share-modal-icon" alt="" src="${escapeAttr(icon)}" data-fallback="logo">
            <div>
                <h2>${escapeHtml(preview.name)}</h2>
                <div class="share-modal-sub">Geteilt von <strong>${escapeHtml(preview.owner_name)}</strong> ·
                    Minecraft ${escapeHtml(preview.minecraft_version)} · ${escapeHtml(preview.loader)} ${escapeHtml(preview.loader_version || '')}</div>
            </div>
        </div>
        ${preview.unpublished_count ? `
            <div class="share-warning"><i class="bi bi-exclamation-triangle-fill"></i>
                ${preview.unpublished_count} Datei(en) sind nicht auf Modrinth veröffentlicht und kommen direkt vom Besitzer.
                Prüfe sie, bevor du annimmst.</div>` : ''}
        <div class="share-modal-section-title">Inhalt (${items.length})</div>
        <div class="share-item-list">${items.map(item => shareItemRow(item)).join('') || '<div class="share-muted">Keine Mods oder Packs</div>'}</div>
        ${preview.configs.length ? `
            <details class="share-configs"><summary>${preview.configs.length} Konfig-Datei(en)</summary>
                ${preview.configs.map(path => `<div class="share-config-path">${escapeHtml(path)}</div>`).join('')}
            </details>` : ''}
        <div class="share-trust-note"><i class="bi bi-shield-exclamation"></i>
            Geteilte Instanzen werden nicht geprüft wie veröffentlichte Projekte. Nimm nur Einladungen von Leuten an, denen du vertraust.</div>
        <div class="share-modal-actions">
            <button class="btn btn-secondary" data-share-decline>${params.link ? 'Abbrechen' : 'Ablehnen'}</button>
            <button class="btn" data-share-accept><i class="bi bi-download"></i> Annehmen & installieren</button>
        </div>
    `;

    modal.querySelector('[data-share-decline]').addEventListener('click', () => {
        modal.remove();
        if (!params.link) declineShareInvite(preview.owner_id, preview.share_id);
    });
    modal.querySelector('[data-share-accept]').addEventListener('click', () => {
        modal.remove();
        acceptShare({
            link: params.link || null,
            ownerId: params.link ? null : preview.owner_id,
            shareId: params.link ? null : preview.share_id
        }, preview.name);
    });
}

async function acceptShare(params, name) {
    setSessionWidgetState({
        profileId: null,
        profileName: name,
        phase: 'spin',
        status: 'Installiere geteilte Instanz…',
        percent: null,
        canStop: false,
        activity: 'import'
    });
    animateSessionFlyIn(name);
    try {
        const result = await invoke('share_accept', params);
        await refreshProfilesInBackground();
        await prepareProfileDownloadFlow(result.profile_id, name, { from: 60, to: 100, activity: 'import' });
        await refreshProfilesInBackground();
        showToast(`"${name}" wurde hinzugefügt`, result.failed.length ? 'warning' : 'success', 4500);
        showImportIssues(result.failed, result.warnings);
    } catch (error) {
        setSessionWidgetState({ phase: 'idle', percent: null, canStop: false, activity: null });
        syncSessionWidgetWithRunningProfiles();
        showToast('Beitreten fehlgeschlagen: ' + error, 'error', 5000);
    }
    refreshFriends();
}

// ==================== Share-Tab im Profil ====================

function renderShareTabShell(profile) {
    return `<div id="share-tab-content" data-profile-id="${escapeAttr(profile.id)}">
        <div class="share-modal-loading"><span class="lion-spin"><i class="bi bi-arrow-repeat"></i></span> Lade…</div>
    </div>`;
}

function refreshOpenShareTab() {
    const container = document.getElementById('share-tab-content');
    if (!container) return;
    // Nicht neu zeichnen, während der Nutzer gerade etwas tut (Dialog offen, Details aufgeklappt, Feld fokussiert)
    const busy = document.getElementById('share-config-modal') ||
        container.querySelector('details[open]') ||
        container.contains(document.activeElement);
    if (!busy) loadShareTab(container.dataset.profileId, { quiet: true });
}

async function loadShareTab(profileId, { quiet = false } = {}) {
    const container = document.getElementById('share-tab-content');
    if (!container || container.dataset.profileId !== profileId) return;

    let status;
    try {
        status = await invoke('share_status', { profileId });
    } catch (error) {
        if (!quiet) container.innerHTML = `<div class="share-card"><span class="lion-spin"><i class="bi bi-arrow-repeat"></i></span> ${escapeHtml(String(error))}</div>`;
        return;
    }
    rememberManagedFiles(profileId, status);

    if (status.role === 'owner') {
        container.innerHTML = renderOwnerShare(profileId, status);
        if (!quiet || !container.dataset.changesLoaded) loadPendingChanges(profileId);
        else container.querySelector('[data-share-changes]').innerHTML = container.dataset.changesHtml || '';
    } else if (status.role === 'member') {
        container.innerHTML = renderMemberShare(profileId, status);
    } else {
        container.innerHTML = `
            <div class="share-card share-intro">
                <i class="bi bi-share-fill share-intro-icon"></i>
                <h3>Instanz mit Freunden teilen</h3>
                <p>Teile Inhalte und ausgewählte Konfig-Dateien dieser Instanz. Wenn du etwas änderst,
                   veröffentlichst du ein Update – deine Freunde sehen beim Spielstart genau, was sich geändert hat,
                   und übernehmen es mit einem Klick.</p>
                <button class="btn" onclick="createShare('${escapeAttr(profileId)}')"><i class="bi bi-share"></i> Instanz teilen</button>
            </div>`;
    }
}

function memberRow(profileId, member, pending) {
    return `
        <div class="friends-row${member.online ? '' : ' is-offline'}">
            <div class="friends-avatar-wrap">
                <img class="friends-avatar" alt="" src="${escapeAttr(playerHeadUrl(member.uuid, member.name))}">
                <span class="friends-dot ${member.online ? 'online' : ''}"></span>
            </div>
            <div class="friends-row-text">
                <div class="friends-name">${escapeHtml(member.name)}</div>
                <div class="friends-sub">${pending ? 'Eingeladen' : (member.online ? 'Online' : 'Offline')}</div>
            </div>
            <div class="friends-row-actions" style="opacity: 1;">
                <button class="friends-icon-btn" onclick="removeShareMember('${escapeAttr(profileId)}', '${escapeAttr(member.id)}')"
                        title="${pending ? 'Einladung zurückziehen' : 'Aus der Instanz entfernen'}" aria-label="Entfernen"><i class="bi bi-x-lg"></i></button>
            </div>
        </div>`;
}

function renderOwnerShare(profileId, status) {
    const pid = escapeAttr(profileId);
    const involved = new Set([...status.members, ...status.invited].map(m => m.id));
    const invitable = (friendsUi.state?.friends || []).filter(f => f.state === 'accepted' && !involved.has(f.id));

    return `
        <div class="share-grid">
            <div class="share-card share-span">
                <div class="share-card-head">
                    <h3><i class="bi bi-arrow-repeat"></i> Updates</h3>
                    <span class="share-muted">Veröffentlicht: Revision ${status.revision}</span>
                </div>
                <div data-share-changes><span class="lion-spin"><i class="bi bi-arrow-repeat"></i></span> Prüfe Änderungen…</div>
            </div>

            <div class="share-card">
                <div class="share-card-head"><h3><i class="bi bi-person-plus"></i> Freunde einladen</h3></div>
                ${invitable.length ? invitable.map(f => `
                    <div class="friends-row${f.online ? '' : ' is-offline-avatar'}" title="${f.online ? '' : 'Offline – die Einladung wird zugestellt, sobald er online ist'}">
                        <div class="friends-avatar-wrap">
                            <img class="friends-avatar" alt="" src="${escapeAttr(playerHeadUrl(f.uuid, f.name))}">
                            <span class="friends-dot ${f.online ? 'online' : ''}"></span>
                        </div>
                        <div class="friends-row-text"><div class="friends-name">${escapeHtml(f.name)}</div></div>
                        <button class="btn btn-secondary share-small-btn" onclick="inviteToShare('${pid}', '${escapeAttr(f.id)}')">Einladen</button>
                    </div>`).join('') : '<div class="share-muted">Alle Freunde sind schon dabei – oder füge rechts neue Freunde hinzu.</div>'}
                <div class="share-link-row">
                    <input type="text" readonly value="${escapeAttr(status.link)}" aria-label="Einladungslink">
                    <button class="btn btn-secondary share-small-btn" onclick="copyToClipboard('${escapeAttr(status.link)}', 'Einladungslink kopiert')"><i class="bi bi-link-45deg"></i> Kopieren</button>
                </div>
                <div class="share-muted" style="font-size: 11px;">Jeder mit diesem Link kann beitreten.</div>
            </div>

            <div class="share-card">
                <div class="share-card-head"><h3><i class="bi bi-people"></i> Mitglieder (${status.members.length})</h3></div>
                ${status.members.map(m => memberRow(profileId, m, false)).join('')}
                ${status.invited.map(m => memberRow(profileId, m, true)).join('')}
                ${!status.members.length && !status.invited.length ? '<div class="share-muted">Noch niemand ist beigetreten.</div>' : ''}
            </div>

            <div class="share-card">
                <div class="share-card-head"><h3><i class="bi bi-file-earmark-code"></i> Konfig-Dateien</h3></div>
                <p class="share-muted">${status.config_files.length
                    ? `${status.config_files.length} Datei(en) werden geteilt.`
                    : 'Es werden noch keine Konfig-Dateien geteilt.'}</p>
                <button class="btn btn-secondary" onclick="openShareConfigPicker('${pid}')"><i class="bi bi-check2-square"></i> Dateien auswählen</button>
            </div>

            <div class="share-card">
                <div class="share-card-head"><h3><i class="bi bi-x-octagon"></i> Teilen beenden</h3></div>
                <p class="share-muted">Mitglieder behalten ihre Instanz, bekommen aber keine Updates mehr.</p>
                <button class="btn btn-secondary share-danger" onclick="stopShare('${pid}')">Teilen beenden</button>
            </div>
        </div>`;
}

function renderDiffSummary(diff) {
    const chips = [];
    if (diff.version_change) chips.push(`<span class="share-chip">${escapeHtml(diff.version_change)}</span>`);
    if (diff.added.length) chips.push(`<span class="share-chip add">+${diff.added.length} neu</span>`);
    if (diff.updated.length) chips.push(`<span class="share-chip upd">${diff.updated.length} aktualisiert</span>`);
    if (diff.removed.length) chips.push(`<span class="share-chip rem">−${diff.removed.length} entfernt</span>`);
    const configs = diff.configs_added.length + diff.configs_changed.length + diff.configs_removed.length;
    if (configs) chips.push(`<span class="share-chip">${configs} Konfig-Datei(en)</span>`);
    return chips.join('');
}

function renderDiffDetails(diff) {
    const configRows = [
        ...diff.configs_added.map(p => `<div class="share-item add"><span class="share-item-kind">Konfig</span><span class="share-item-name">${escapeHtml(p)}</span><span class="share-item-version">neu</span></div>`),
        ...diff.configs_changed.map(p => `<div class="share-item upd"><span class="share-item-kind">Konfig</span><span class="share-item-name">${escapeHtml(p)}</span><span class="share-item-version">geändert</span></div>`),
        ...diff.configs_removed.map(p => `<div class="share-item rem"><span class="share-item-kind">Konfig</span><span class="share-item-name">${escapeHtml(p)}</span><span class="share-item-version">nicht mehr geteilt</span></div>`)
    ];
    return `
        ${diff.version_change ? `<div class="share-warning"><i class="bi bi-exclamation-triangle-fill"></i> Version/Loader ändert sich: ${escapeHtml(diff.version_change)}</div>` : ''}
        <div class="share-item-list">
            ${diff.added.map(i => shareItemRow(i, 'add')).join('')}
            ${diff.updated.map(i => shareItemRow(i, 'upd')).join('')}
            ${diff.removed.map(i => shareItemRow(i, 'rem')).join('')}
            ${configRows.join('')}
        </div>`;
}

async function loadPendingChanges(profileId) {
    const container = document.getElementById('share-tab-content');
    const target = container?.querySelector('[data-share-changes]');
    if (!target) return;
    let changes;
    try {
        changes = await invoke('share_pending_changes', { profileId });
    } catch (error) {
        target.innerHTML = `<div class="share-muted">Änderungen konnten nicht geprüft werden: ${escapeHtml(String(error))}</div>`;
        return;
    }
    const pid = escapeAttr(profileId);
    const html = changes.ahead ? `
        <div class="share-ahead"><i class="bi bi-cloud-arrow-up-fill"></i> Deine Instanz ist deinen Freunden voraus</div>
        <div class="share-chips">${renderDiffSummary(changes.diff)}</div>
        <details class="share-configs"><summary>Änderungen anzeigen</summary>${renderDiffDetails(changes.diff)}</details>
        <div class="share-actions">
            <button class="btn" onclick="pushShare('${pid}', 'all')"><i class="bi bi-cloud-upload"></i> Alle Änderungen veröffentlichen</button>
            ${changes.configs_changed && changes.content_changed ? `<button class="btn btn-secondary" onclick="pushShare('${pid}', 'configs')">Nur Konfig-Dateien veröffentlichen</button>` : ''}
        </div>`
        : `<div class="share-uptodate"><i class="bi bi-check-circle-fill"></i> Alle Mitglieder sind auf deinem aktuellen Stand.</div>`;
    const current = document.getElementById('share-tab-content');
    if (current) {
        current.dataset.changesLoaded = '1';
        current.dataset.changesHtml = html;
        const slot = current.querySelector('[data-share-changes]');
        if (slot) slot.innerHTML = html;
    }
}

function renderMemberShare(profileId, status) {
    const pid = escapeAttr(profileId);
    const updateAvailable = status.latest_revision > status.revision;
    return `
        <div class="share-grid">
            <div class="share-card share-span">
                <div class="share-card-head">
                    <h3><i class="bi bi-share-fill"></i> Geteilte Instanz</h3>
                    <span class="share-muted">Revision ${status.revision}</span>
                </div>
                <div class="friends-row${status.owner_online ? '' : ' is-offline'}">
                    <div class="friends-avatar-wrap">
                        <img class="friends-avatar" alt="" src="${escapeAttr(playerHeadUrl(null, status.owner_name))}">
                        <span class="friends-dot ${status.owner_online ? 'online' : ''}"></span>
                    </div>
                    <div class="friends-row-text">
                        <div class="friends-name">${escapeHtml(status.owner_name)}</div>
                        <div class="friends-sub">Besitzer · ${status.owner_online ? 'Online' : 'Offline'}</div>
                    </div>
                </div>
                ${status.ended
                    ? `<div class="share-warning"><i class="bi bi-info-circle-fill"></i> ${escapeHtml(status.owner_name)} teilt diese Instanz nicht mehr. Sie bleibt als normale Instanz erhalten.</div>`
                    : updateAvailable
                        ? `<div class="share-ahead"><i class="bi bi-cloud-arrow-down-fill"></i> Update verfügbar (Revision ${status.latest_revision})</div>`
                        : ''}
                <div class="share-actions">
                    ${status.ended ? '' : `<button class="btn${updateAvailable ? '' : ' btn-secondary'}" onclick="checkSharedUpdateManually('${pid}')">
                        <i class="bi bi-arrow-repeat"></i> ${updateAvailable ? 'Update ansehen' : 'Nach Updates suchen'}</button>`}
                    <button class="btn btn-secondary share-danger" onclick="leaveShare('${pid}')">${status.ended ? 'Aus geteilten Instanzen entfernen' : 'Instanz verlassen'}</button>
                </div>
            </div>
            <div class="share-card share-span share-muted">
                <i class="bi bi-lock-fill"></i> Inhalte des Besitzers sind gesperrt. Du kannst eigene Inhalte hinzufügen –
                Änderungen an Minecraft-Version oder Loader durch den Besitzer können diese aber beschädigen.
                Updates werden beim Spielstart geprüft.
            </div>
        </div>`;
}

function rememberManagedFiles(profileId, status) {
    if (status.role === 'member') {
        sharedManagedFiles.set(profileId, {
            owner: status.owner_name,
            mods: new Set(status.managed_mods)
        });
    } else {
        sharedManagedFiles.delete(profileId);
    }
}

/** Lädt beim Öffnen eines Profils, welche Mods vom Besitzer verwaltet (gesperrt) sind. */
async function loadSharedManagedFiles(profileId) {
    try {
        const status = await invoke('share_status', { profileId });
        const hadLock = sharedManagedFiles.has(profileId);
        rememberManagedFiles(profileId, status);
        if (status.role === 'member' || hadLock) loadInstalledMods(profileId, { force: true });
    } catch (_e) {
        // Freunde-System noch nicht bereit
    }
}

/** Gehört die Mod-Datei zur geteilten Instanz des Besitzers? */
function isManagedSharedMod(profileId, filename) {
    const managed = sharedManagedFiles.get(profileId);
    if (!managed) return null;
    const base = String(filename).replace(/\.disabled$/, '');
    return managed.mods.has(base) ? managed.owner : null;
}

async function createShare(profileId) {
    const container = document.getElementById('share-tab-content');
    if (container) container.innerHTML = `<div class="share-card"><span class="lion-spin"><i class="bi bi-arrow-repeat"></i></span> Bereite Instanz zum Teilen vor…</div>`;
    try {
        await invoke('share_create', { profileId });
        showToast('Instanz wird jetzt geteilt – lade deine Freunde ein', 'success', 3500);
    } catch (error) {
        showToast('Teilen fehlgeschlagen: ' + error, 'error', 5000);
    }
    loadShareTab(profileId);
}

async function pushShare(profileId, mode) {
    const slot = document.querySelector('#share-tab-content [data-share-changes]');
    if (slot) slot.innerHTML = `<span class="lion-spin"><i class="bi bi-arrow-repeat"></i></span> Veröffentliche…`;
    try {
        await invoke('share_push', { profileId, mode });
        showToast(mode === 'configs' ? 'Konfig-Dateien veröffentlicht' : 'Änderungen veröffentlicht', 'success', 3000);
    } catch (error) {
        showToast('Veröffentlichen fehlgeschlagen: ' + error, 'error', 5000);
    }
    loadShareTab(profileId);
}

async function inviteToShare(profileId, friendId) {
    try {
        const message = await invoke('share_invite_friend', { profileId, friendId });
        showToast(message, 'success', 3500);
    } catch (error) {
        showToast(String(error), 'error', 4000);
    }
    loadShareTab(profileId, { quiet: true });
}

async function removeShareMember(profileId, memberId) {
    try {
        await invoke('share_remove_member', { profileId, memberId });
    } catch (error) {
        showToast(String(error), 'error', 4000);
    }
    loadShareTab(profileId, { quiet: true });
}

async function stopShare(profileId) {
    const confirmed = await showConfirmModal({
        title: 'Teilen beenden',
        message: 'Möchtest du das Teilen dieser Instanz wirklich beenden',
        details: 'Mitglieder behalten ihre Instanz, bekommen aber keine Updates mehr.',
        confirmText: 'Teilen beenden',
        icon: 'bi-x-octagon'
    });
    if (!confirmed) return;
    try {
        await invoke('share_stop', { profileId });
        showToast('Teilen beendet', 'info', 2500);
    } catch (error) {
        showToast(String(error), 'error', 4000);
    }
    loadShareTab(profileId);
}

async function leaveShare(profileId) {
    const confirmed = await showConfirmModal({
        title: 'Instanz verlassen',
        message: 'Möchtest du diese geteilte Instanz verlassen',
        details: 'Die Instanz bleibt als normale Instanz erhalten, bekommt aber keine Updates mehr.',
        confirmText: 'Verlassen',
        icon: 'bi-box-arrow-left'
    });
    if (!confirmed) return;
    try {
        await invoke('share_leave', { profileId });
        showToast('Geteilte Instanz verlassen', 'info', 2500);
    } catch (error) {
        showToast(String(error), 'error', 4000);
    }
    sharedManagedFiles.delete(profileId);
    loadShareTab(profileId);
}

async function openShareConfigPicker(profileId) {
    const modal = shareModalShell('share-config-modal', `
        <div class="share-modal-loading"><span class="lion-spin"><i class="bi bi-arrow-repeat"></i></span> Lade Konfig-Dateien…</div>`);
    let files;
    try {
        files = await invoke('share_list_config_files', { profileId });
    } catch (error) {
        modal.remove();
        showToast(String(error), 'error', 4000);
        return;
    }
    modal.querySelector('.share-modal').innerHTML = `
        <div class="share-modal-head"><div><h2><i class="bi bi-file-earmark-code"></i> Konfig-Dateien teilen</h2>
            <div class="share-modal-sub">Ausgewählte Dateien werden beim nächsten Veröffentlichen an deine Freunde übertragen.</div></div></div>
        <input type="text" class="share-config-filter" placeholder="Dateien filtern…" aria-label="Dateien filtern" data-config-filter>
        <div class="share-config-list" data-config-list>
            ${files.length ? files.map(f => `
                <label class="share-config-row" data-path="${escapeAttr(f.path.toLowerCase())}">
                    <input type="checkbox" value="${escapeAttr(f.path)}" ${f.selected ? 'checked' : ''}>
                    <span class="share-config-path-text">${escapeHtml(f.path)}</span>
                    <span class="share-muted">${(f.size / 1024).toFixed(1)} KB</span>
                </label>`).join('') : '<div class="share-muted">Keine Konfig-Dateien gefunden (config/, defaultconfigs/, kubejs/, options.txt).</div>'}
        </div>
        <div class="share-modal-actions">
            <button class="btn btn-secondary" data-config-cancel>Abbrechen</button>
            <button class="btn" data-config-save>Speichern</button>
        </div>`;

    modal.querySelector('[data-config-filter]').addEventListener('input', (event) => {
        const query = event.target.value.toLowerCase();
        modal.querySelectorAll('.share-config-row').forEach(row => {
            row.classList.toggle('hidden', !!query && !row.dataset.path.includes(query));
        });
    });
    modal.querySelector('[data-config-cancel]').addEventListener('click', () => modal.remove());
    modal.querySelector('[data-config-save]').addEventListener('click', async () => {
        const paths = Array.from(modal.querySelectorAll('[data-config-list] input:checked')).map(input => input.value);
        try {
            await invoke('share_set_config_files', { profileId, paths });
            showToast('Auswahl gespeichert – veröffentliche ein Update, um sie zu teilen', 'success', 3500);
        } catch (error) {
            showToast(String(error), 'error', 4000);
        }
        modal.remove();
        loadShareTab(profileId);
    });
}

// ==================== Updates beim Spielstart ====================

/** Zeigt die Änderungen eines Updates. Ergebnis: 'apply' | 'skip' | 'cancel'. */
function showShareUpdateReview(check, ownerName) {
    return new Promise((resolve) => {
        const modal = shareModalShell('share-update-modal', `
            <div class="share-modal-head"><div>
                <h2><i class="bi bi-cloud-arrow-down"></i> Update für geteilte Instanz</h2>
                <div class="share-modal-sub">${escapeHtml(ownerName)} hat Änderungen veröffentlicht (Revision ${check.revision} → ${check.latest_revision}).</div>
            </div></div>
            <div class="share-chips">${renderDiffSummary(check.diff)}</div>
            ${renderDiffDetails(check.diff)}
            ${[...check.diff.added, ...check.diff.updated].some(i => i.unpublished) ? `
                <div class="share-warning"><i class="bi bi-exclamation-triangle-fill"></i>
                    Das Update enthält Dateien, die nicht auf Modrinth veröffentlicht sind. Übernimm es nur, wenn du dem Besitzer vertraust.</div>` : ''}
            <div class="share-modal-actions">
                <button class="btn btn-secondary" data-update-cancel>Abbrechen</button>
                <button class="btn btn-secondary" data-update-skip>Ohne Update spielen</button>
                <button class="btn" data-update-apply><i class="bi bi-check-lg"></i> Übernehmen</button>
            </div>`);
        const finish = (result) => {
            modal.remove();
            resolve(result);
        };
        // Klick daneben = abbrechen
        modal.addEventListener('click', () => resolve('cancel'), { once: true });
        modal.querySelector('[data-update-cancel]').addEventListener('click', () => finish('cancel'));
        modal.querySelector('[data-update-skip]').addEventListener('click', () => finish('skip'));
        modal.querySelector('[data-update-apply]').addEventListener('click', () => finish('apply'));
    });
}

async function applySharedUpdate(profileId) {
    const profile = profiles.find(p => p.id === profileId);
    setSessionWidgetState({
        profileId,
        profileName: profile?.name || 'Instanz',
        phase: 'spin',
        status: 'Übernehme Update…',
        percent: null,
        canStop: false,
        activity: null
    });
    const result = await invoke('share_apply_update', { profileId });
    await refreshProfilesInBackground();
    invalidateProfileContentCaches(profileId);
    showToast('Update übernommen', result.failed.length ? 'warning' : 'success', 3000);
    showImportIssues(result.failed, result.warnings);
}

/**
 * Wird vor jedem Spielstart aufgerufen. Gibt false zurück, wenn der Start abgebrochen werden soll.
 */
async function syncSharedInstanceBeforeLaunch(profileId) {
    let status;
    try {
        status = await invoke('share_status', { profileId });
    } catch (_e) {
        return true; // Freunde-System nicht bereit → normal starten
    }
    if (status.role !== 'member' || status.ended) return true;

    let check;
    try {
        check = await invoke('share_check_update', { profileId });
    } catch (error) {
        showToast('Updates konnten nicht geprüft werden: ' + error, 'warning', 4000);
        return true;
    }
    if (!check.available) {
        if (!check.owner_online) {
            showToast(`${status.owner_name} ist offline – Updates können gerade nicht geprüft werden`, 'info', 3500);
        }
        return true;
    }

    const decision = await showShareUpdateReview(check, status.owner_name);
    if (decision === 'cancel') return false;
    if (decision === 'apply') {
        try {
            await applySharedUpdate(profileId);
        } catch (error) {
            showToast('Update fehlgeschlagen: ' + error, 'error', 5000);
            return false;
        }
    }
    return true;
}

async function checkSharedUpdateManually(profileId) {
    let status;
    let check;
    try {
        status = await invoke('share_status', { profileId });
        check = await invoke('share_check_update', { profileId });
    } catch (error) {
        showToast(String(error), 'error', 4000);
        return;
    }
    if (!check.available) {
        showToast(check.message || 'Du bist auf dem neuesten Stand', check.owner_online ? 'success' : 'info', 3500);
        loadShareTab(profileId, { quiet: true });
        return;
    }
    if (await showShareUpdateReview(check, status.owner_name) === 'apply') {
        try {
            await applySharedUpdate(profileId);
        } catch (error) {
            showToast('Update fehlgeschlagen: ' + error, 'error', 5000);
        }
    }
    loadShareTab(profileId);
}

document.addEventListener('DOMContentLoaded', initFriends);

// Launcher-Updates über GitHub-Releases.
//
// * Prüft beim Start und danach alle paar Stunden.
// * Gefundenes Update: gelber Punkt am Settings-Symbol, "Update verfügbar" im Aktivitäts-Kasten
//   und ein Update-Button oben in den Einstellungen.
// * Der Button lädt das passende Paket, prüft die Prüfsumme und installiert es.
//
// Nutzt aus app.js: invoke, showToast, escapeHtml, openExternalUrl, setSessionWidgetState,
// syncSessionWidgetWithRunningProfiles, formatMarkdown.

const UPDATE_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

const launcherUpdate = {
    info: null,
    checking: false,
    installing: false,
    progress: null,
    error: null
};

function isLauncherUpdateAvailable() {
    return !!launcherUpdate.info?.available;
}

function formatBytes(bytes) {
    if (!bytes) return '';
    return bytes > 1024 * 1024 ? `${(bytes / 1024 / 1024).toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`;
}

async function checkLauncherUpdate({ manual = false } = {}) {
    if (launcherUpdate.checking || launcherUpdate.installing) return;
    launcherUpdate.checking = true;
    launcherUpdate.error = null;
    renderLauncherUpdate();
    try {
        launcherUpdate.info = await invoke('updater_check');
        if (manual && !launcherUpdate.info.available) {
            showToast(launcherUpdate.info.hint || 'Der Launcher ist auf dem neuesten Stand', 'success', 3000);
        }
    } catch (error) {
        launcherUpdate.error = String(error);
        if (manual) showToast('Update-Prüfung fehlgeschlagen: ' + error, 'error', 4500);
    } finally {
        launcherUpdate.checking = false;
        renderLauncherUpdate();
        syncSessionWidgetWithRunningProfiles();
    }
}

async function installLauncherUpdate() {
    const info = launcherUpdate.info;
    if (!info?.can_install || launcherUpdate.installing) return;

    launcherUpdate.installing = true;
    launcherUpdate.progress = 0;
    renderLauncherUpdate();

    let unlisten = null;
    try {
        unlisten = await window.__TAURI__.event.listen('updater-progress', (event) => {
            const { downloaded, total } = event.payload || {};
            const percent = total ? Math.round((downloaded / total) * 100) : null;
            launcherUpdate.progress = percent;
            setSessionWidgetState({
                profileId: null,
                profileName: 'Launcher-Update',
                phase: 'progress',
                status: `Update wird heruntergeladen… ${percent ?? ''}%`,
                percent,
                canStop: false,
                activity: 'update'
            });
            renderLauncherUpdate();
        });
    } catch (_e) {
        // Ohne Event-API eben ohne Fortschrittsanzeige
    }

    try {
        await invoke('updater_install');
        // Der Launcher beendet sich gleich und der Installer bzw. die neue Version startet
        setSessionWidgetState({
            profileId: null,
            profileName: 'Launcher-Update',
            phase: 'spin',
            status: 'Update wird installiert – der Launcher startet neu…',
            percent: null,
            canStop: false,
            activity: 'update'
        });
        showToast('Update wird installiert – der Launcher startet neu', 'success', 5000);
    } catch (error) {
        launcherUpdate.installing = false;
        launcherUpdate.error = String(error);
        setSessionWidgetState({ phase: 'idle', percent: null, canStop: false, activity: null });
        syncSessionWidgetWithRunningProfiles();
        showToast('Update fehlgeschlagen: ' + error, 'error', 6000);
    } finally {
        if (typeof unlisten === 'function') unlisten();
        renderLauncherUpdate();
    }
}

function renderLauncherUpdate() {
    // Gelber Punkt am Settings-Symbol
    document.querySelector('.nav-item[data-page="settings"]')
        ?.classList.toggle('has-update', isLauncherUpdateAvailable());

    const card = document.getElementById('launcher-update-card');
    if (!card) return;
    const info = launcherUpdate.info;
    const current = info
        ? `Version ${escapeHtml(info.current_version)}${info.commit ? ` · ${escapeHtml(info.commit)}` : ''} · Kanal: ${escapeHtml(info.channel)}`
        : '';

    let body;
    if (launcherUpdate.installing) {
        const percent = launcherUpdate.progress;
        body = `
            <div class="launcher-update-status available"><span class="lion-spin"><i class="bi bi-arrow-repeat"></i></span>
                Update wird heruntergeladen… ${percent != null ? percent + '%' : ''}</div>
            <div class="launcher-update-bar"><div style="width: ${percent || 0}%"></div></div>`;
    } else if (launcherUpdate.checking) {
        body = `<div class="launcher-update-status"><span class="lion-spin"><i class="bi bi-arrow-repeat"></i></span> Suche nach Updates…</div>`;
    } else if (info?.available) {
        const published = info.published_at ? new Date(info.published_at).toLocaleDateString() : '';
        body = `
            <div class="launcher-update-status available"><i class="bi bi-stars"></i>
                Neue Version verfügbar: <strong>${escapeHtml(info.latest_label || '')}</strong>${published ? ` <span class="share-muted">(${escapeHtml(published)})</span>` : ''}</div>
            ${info.notes ? `<details class="launcher-update-notes"><summary>Änderungen</summary><div class="project-body">${formatMarkdown(info.notes)}</div></details>` : ''}
            <div class="share-actions">
                ${info.can_install
                    ? `<button class="btn" onclick="installLauncherUpdate()"><i class="bi bi-download"></i> Jetzt aktualisieren${info.asset_size ? ` (${formatBytes(info.asset_size)})` : ''}</button>`
                    : ''}
                ${info.release_url ? `<button class="btn btn-secondary" onclick="openExternalUrl('${escapeAttr(info.release_url)}')"><i class="bi bi-box-arrow-up-right"></i> Release-Seite</button>` : ''}
            </div>
            ${info.hint ? `<div class="share-muted">${escapeHtml(info.hint)}</div>` : ''}`;
    } else {
        body = `
            <div class="launcher-update-status ok"><i class="bi bi-check-circle-fill"></i>
                ${info ? escapeHtml(info.hint || 'Der Launcher ist auf dem neuesten Stand') : 'Noch nicht geprüft'}</div>
            ${launcherUpdate.error ? `<div class="share-muted">Letzte Prüfung fehlgeschlagen: ${escapeHtml(launcherUpdate.error)}</div>` : ''}
            <div class="share-actions">
                <button class="btn btn-secondary" onclick="checkLauncherUpdate({ manual: true })"><i class="bi bi-arrow-repeat"></i> Nach Updates suchen</button>
            </div>`;
    }

    card.innerHTML = `
        <div class="launcher-update-head">
            <h3><i class="bi bi-cloud-arrow-down"></i> Launcher-Updates</h3>
            <span class="share-muted">${current}</span>
        </div>
        ${body}`;
}

document.addEventListener('DOMContentLoaded', () => {
    renderLauncherUpdate();
    // Etwas warten, damit der Start nicht ausgebremst wird
    setTimeout(() => checkLauncherUpdate(), 5000);
    setInterval(() => checkLauncherUpdate(), UPDATE_CHECK_INTERVAL_MS);
});

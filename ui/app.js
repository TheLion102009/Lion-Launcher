// ── Bild-Fallbacks ───────────────────────────────────────────────────────────
// Bilder bekommen statt Inline-onerror-Handlern ein data-fallback-Attribut
// (z.B. <img data-fallback="icon">). Ein zentraler Listener zeigt bei Ladefehlern den Ersatz.
function handleIconError(el) {
    el.parentElement.innerHTML = '<span style="font-size: 22px;"><i class="bi bi-box"></i></span>';
}
function handleIconErrorLarge(el) {
    el.parentElement.innerHTML = '<div style="font-size: 48px; display: flex; align-items: center; justify-content: center; width: 100%; height: 100%; background: var(--bg-dark); border-radius: 8px;"><i class="bi bi-box"></i></div>';
}
function handleIconErrorMedium(el) {
    const iconClass = el.dataset.fallbackIcon || 'bi-box';
    el.parentElement.innerHTML = '<div style="font-size: 32px; display: flex; align-items: center; justify-content: center; width: 100%; height: 100%;"><i class="bi ' + iconClass + '"></i></div>';
}

function handleGalleryImageError(el) {
    const wrapper = el.parentElement;
    if (!wrapper) return;
    wrapper.innerHTML = `
        <div style="height: 180px; display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px; background: var(--bg-dark); color: var(--text-secondary);">
            <i class="bi bi-image" style="font-size: 28px;"></i>
            <span style="font-size: 12px;">Image could not be loaded</span>
        </div>
    `;
}

function handleFullscreenImageError(el) {
    const container = el.parentElement;
    if (!container) return;
    container.innerHTML = `
        <div style="display:flex;flex-direction:column;align-items:center;justify-content:center;color:var(--text-secondary);padding:40px;text-align:center;">
            <i class="bi bi-image" style="font-size:42px;margin-bottom:10px;"></i>
            <span>The image could not be loaded.</span>
        </div>
    `;
}

const IMAGE_FALLBACKS = {
    // Profil-Icon → Launcher-Logo
    logo: img => { img.src = LION_LOGO_URL; },
    icon: handleIconError,
    'icon-medium': handleIconErrorMedium,
    'icon-large': handleIconErrorLarge,
    gallery: handleGalleryImageError,
    fullscreen: handleFullscreenImageError,
    // Bild ausblenden und das direkt folgende Ersatz-Element anzeigen
    sibling: img => {
        img.style.display = 'none';
        if (img.nextElementSibling) img.nextElementSibling.style.display = 'flex';
    }
};

// error-Events von Bildern steigen nicht auf → in der Capture-Phase abfangen
document.addEventListener('error', (event) => {
    const img = event.target;
    if (!(img instanceof HTMLImageElement) || !img.dataset.fallback) return;
    const fallback = IMAGE_FALLBACKS[img.dataset.fallback];
    // Nur einmal auslösen, damit ein ebenfalls fehlendes Ersatzbild keine Schleife erzeugt
    delete img.dataset.fallback;
    if (fallback) fallback(img);
}, true);

function normalizeMediaUrl(url) {
    if (!url) return '';
    const value = String(url).trim();
    if (!value) return '';
    if (value.startsWith('data:image/')) return value;
    if (/^https?:\/\//i.test(value)) return value;
    if (value.startsWith('//')) return 'https:' + value;
    return value;
}

/**
 * Escapes special HTML characters for use in HTML attributes.
 * Prevents the ';">' bug when mod names contain quotes.
 */
function escapeAttr(str) {
    if (!str) return '';
    return String(str)
        .replace(/&/g, '&amp;')
        .replace(/"/g, '&quot;')
        .replace(/'/g, '&#39;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;');
}

function resolveUiAssetUrl(fileName) {
    try {
        return new URL(fileName, document.baseURI).href;
    } catch (_e) {
        return fileName;
    }
}

let LION_LOGO_URL = resolveUiAssetUrl('icon.png');

async function resolveEmbeddedLogoUrl() {
    try {
        const dataUrl = await invoke('get_embedded_logo_data_url');
        if (typeof dataUrl === 'string' && dataUrl.startsWith('data:image/png;base64,')) {
            LION_LOGO_URL = dataUrl;
        }
    } catch (_e) {
        // Fallback remains the local asset URL
    }
}

function initSidebarLogo() {
    const sidebarLogo = document.querySelector('.sidebar-logo-img');
    if (!sidebarLogo) return;

    sidebarLogo.src = LION_LOGO_URL;
    sidebarLogo.onerror = () => {
        sidebarLogo.onerror = null;
        sidebarLogo.style.display = 'none';
    };
}

// Debug log function for visual feedback
function debugLog(message, type = 'info') {
    const time = new Date().toLocaleTimeString();
    console.log(`[${time}] [${type}] ${message}`);
}

// Disable right-click context menu (except for special elements)
document.addEventListener('contextmenu', (e) => {
    // Allow right-click only on elements with the data-context-menu attribute
    if (!e.target.closest('[data-context-menu]')) {
        e.preventDefault();
    }
});

// Tauri 2 API Import - mit Fallback
function resolveTauriInvoke() {
    if (window.__TAURI_INTERNALS__) {
        debugLog('Tauri 2 API loaded (__TAURI_INTERNALS__)', 'success');
        return window.__TAURI_INTERNALS__.invoke;
    }
    if (window.__TAURI__?.core) {
        debugLog('Tauri 2 API loaded (__TAURI__.core)', 'success');
        return window.__TAURI__.core.invoke;
    }
    if (window.__TAURI__?.tauri) {
        debugLog('Tauri 1 API loaded (fallback)', 'success');
        return window.__TAURI__.tauri.invoke;
    }
    console.error('Failed to load Tauri API: Tauri API not found!');
    return async (cmd) => {
        console.error('Mock invoke:', cmd);
        throw new Error('Tauri API not available');
    };
}

const invoke = resolveTauriInvoke();

// ── Externe Links ────────────────────────────────────────────────────────────
// Externe Links (Modrinth, GitHub, Discord, ...) werden IMMER im System-Browser geöffnet,
// nie im Launcher-Fenster. Das Backend blockiert zusätzlich jede Navigation weg vom Launcher.
function getExternalUrl(href) {
    if (!href) return null;
    let url;
    try {
        url = new URL(href, window.location.href);
    } catch (_e) {
        return null;
    }
    if (!['http:', 'https:', 'mailto:'].includes(url.protocol)) return null;
    if (url.protocol !== 'mailto:' && url.origin === window.location.origin) return null;
    return url.href;
}

async function openExternalUrl(href) {
    const url = getExternalUrl(href);
    if (!url) return;
    try {
        await invoke('open_external_url', { url });
    } catch (error) {
        debugLog('Failed to open external link: ' + error, 'error');
        if (typeof showToast === 'function') showToast('Link konnte nicht geöffnet werden: ' + error, 'error', 3500);
    }
}

function handleExternalLinkEvent(event) {
    // auxclick: nur Mittelklick (öffnet sonst ein neues Fenster im Launcher)
    if (event.type === 'auxclick' && event.button !== 1) return;
    const anchor = event.target?.closest?.('a[href]');
    if (!anchor) return;
    const url = getExternalUrl(anchor.getAttribute('href'));
    if (!url) return;
    event.preventDefault();
    event.stopPropagation();
    openExternalUrl(url);
}

// Capture-Phase: läuft vor allen anderen Handlern (auch vor dem des Shell-Plugins)
document.addEventListener('click', handleExternalLinkEvent, true);
document.addEventListener('auxclick', handleExternalLinkEvent, true);

// ── Keine nativen Browser-Dialoge ("tauri.localhost says ...") ─────────────────
// Meldungen laufen über Toasts, Rückfragen über showConfirmModal/showPromptModal.
// Falls irgendwo doch noch alert/confirm/prompt aufgerufen wird, erscheint trotzdem
// nie das native Fenster.
window.alert = (message) => {
    showToast(String(message ?? ''), 'info', 4500);
};
window.confirm = (message) => {
    console.warn('Native confirm() blocked – use showConfirmModal():', message);
    return false;
};
window.prompt = (message) => {
    console.warn('Native prompt() blocked – use showPromptModal():', message);
    return null;
};

// State
let currentPage = 'profiles';
let currentProfile = null;
let profiles = [];
let galleryModalKeyHandler = null;
let currentUsername = 'Guest';
let openedFromProfile = false;
let skipLoadProfiles = false;
let currentProfileSubTab = 'mods';
let selectedFilters = {
    version: '',
    loader: '',
    sort: 'downloads',
    categories: [],
    hideInstalled: false
};
// v2: Mods/Packs mit exakter Version (version_id + Hash) bzw. eingebetteter Datei
const LION_SHARE_FORMAT_VERSION = 2;
const LION_CONTENT_KINDS = ['mods', 'resourcepacks', 'shaderpacks'];
const LION_CONTENT_LABELS = { mods: 'mods', resourcepacks: 'resource packs', shaderpacks: 'shader packs' };
let pendingLionImportPayload = null;
let currentModSearchQuery = '';
let currentModPage = 0;
const MODS_PER_PAGE = 20;
let currentContentType = 'mods';
let serverProfiles = [];
let currentServerProfile = null;
let serverHubPollInterval = null;
let serverConsolePollInterval = null;
let serverDetailsPollInterval = null;
let currentServerFilesPath = '';
let currentServerEditedFile = null;
const serverMetricHistory = new Map();
let serverConsoleAutoScroll = true;
const DEFAULT_SERVER_JAVA_ARGS = [
    '-XX:+UnlockExperimentalVMOptions',
    '-XX:+UseG1GC',
    '-XX:G1NewSizePercent=20',
    '-XX:G1ReservePercent=20',
    '-XX:MaxGCPauseMillis=50',
    '-XX:G1HeapRegionSize=32M',
].join(' ');
let latestStableJavaMajor = 21;
let javaRuntimeOptionsCache = [];

// Calculate the effective limit: when "Hide Installed" is on, load more to fill the page
function getEffectiveLimit() {
    return selectedFilters.hideInstalled ? MODS_PER_PAGE * 5 : MODS_PER_PAGE;
}

// Theme State
let currentTheme = 'dark';
let currentAccentColor = 'gold';

// ── Running Instances State ──────────────────────────────────────────────────
/** Set of all currently running profile IDs */
const runningProfiles = new Set();

const sessionWidgetState = {
    profileId: null,
    profileName: 'No Session Active',
    phase: 'idle',
    status: 'Idle',
    percent: null,
    canStop: false,
    // z.B. 'import': Aktivität ohne (bzw. vor der) Profil-ID, die nicht überschrieben werden darf
    activity: null
};
let sessionWidgetInitialized = false;

function getProfileNameById(profileId) {
    if (!profileId) return 'No Session Active';
    const profile = profiles.find(p => p.id === profileId);
    return profile ? profile.name : 'Unknown Profile';
}

function setSessionWidgetState(nextState = {}) {
    Object.assign(sessionWidgetState, nextState);
    renderSessionWidget();
}

function syncSessionWidgetWithRunningProfiles() {
    // Ein laufender Start, Download oder Import hat Vorrang vor der Anzeige laufender Instanzen
    const hasActiveTransientState = !!(sessionWidgetState.profileId || sessionWidgetState.activity) && (
        sessionWidgetState.phase === 'spin' ||
        sessionWidgetState.phase === 'progress' ||
        sessionWidgetState.phase === 'queued'
    );
    if (hasActiveTransientState) {
        return;
    }

    if (runningProfiles.size === 0) {
        setSessionWidgetState({
            profileId: null,
            profileName: 'No Session Active',
            phase: 'idle',
            status: 'Idle',
            percent: null,
            canStop: false,
            activity: null
        });
        return;
    }

    const keepCurrent = sessionWidgetState.profileId && runningProfiles.has(sessionWidgetState.profileId);
    const activeProfileId = keepCurrent ? sessionWidgetState.profileId : runningProfiles.values().next().value;
    setSessionWidgetState({
        profileId: activeProfileId,
        profileName: getProfileNameById(activeProfileId),
        phase: 'running',
        status: 'Minecraft gestartet',
        percent: null,
        canStop: true,
        activity: null
    });
}

function renderSessionWidget() {
    const widget = document.getElementById('session-widget');
    const indicator = document.getElementById('session-indicator');
    const indicatorLabel = document.getElementById('session-indicator-label');
    const nameEl = document.getElementById('session-profile-name');
    const stopBtn = document.getElementById('session-stop-btn');
    if (!widget || !indicator || !indicatorLabel || !nameEl || !stopBtn) return;

    const phase = sessionWidgetState.phase || 'idle';
    const safePercent = Number.isFinite(sessionWidgetState.percent)
        ? Math.max(0, Math.min(100, Math.round(sessionWidgetState.percent)))
        : null;

    indicator.classList.remove('idle', 'spin', 'progress', 'running', 'queued');
    indicator.classList.add(phase);
    if (safePercent != null) {
        indicator.style.setProperty('--session-progress', String(safePercent));
    } else {
        indicator.style.setProperty('--session-progress', '0');
    }

    if (phase === 'progress' && safePercent != null && safePercent < 100) {
        indicatorLabel.textContent = safePercent + '%';
    } else {
        indicatorLabel.textContent = '';
    }

    nameEl.textContent = sessionWidgetState.profileName || 'No Session Active';

    // Statuszeile (z.B. "Importing mods... (3/40)") nur während laufender Aktivitäten
    const statusEl = document.getElementById('session-status');
    const isBusy = phase === 'spin' || phase === 'progress' || phase === 'queued';
    const statusText = isBusy ? String(sessionWidgetState.status || '') : '';
    if (statusEl) {
        statusEl.textContent = statusText;
        statusEl.classList.toggle('hidden', !statusText);
    }
    widget.title = statusText;

    const canStop = !!sessionWidgetState.profileId && !!sessionWidgetState.canStop;
    stopBtn.classList.toggle('hidden', !canStop);
}

function initSessionWidget() {
    if (sessionWidgetInitialized) return;
    sessionWidgetInitialized = true;

    const stopBtn = document.getElementById('session-stop-btn');
    if (stopBtn) {
        stopBtn.addEventListener('click', (e) => {
            e.preventDefault();
            e.stopPropagation();
            if (sessionWidgetState.profileId) {
                stopProfile(sessionWidgetState.profileId);
            }
        });
    }

    renderSessionWidget();
}

function animateSessionFlyIn(profileName) {
    const widget = document.getElementById('session-widget');
    if (!widget) return;

    widget.classList.remove('fly-in-highlight');
    // Trigger reflow to restart pulse animation reliably.
    void widget.offsetWidth;
    widget.classList.add('fly-in-highlight');

    const chip = document.createElement('div');
    chip.className = 'session-fly-chip';
    chip.textContent = profileName;
    document.body.appendChild(chip);

    const startX = Math.max(window.innerWidth * 0.5 - 70, 20);
    const startY = Math.max(window.innerHeight * 0.5 - 16, 20);
    const widgetRect = widget.getBoundingClientRect();
    const targetX = widgetRect.left + 16;
    const targetY = widgetRect.top + 24;

    chip.style.left = startX + 'px';
    chip.style.top = startY + 'px';
    chip.animate(
        [
            { transform: 'translate(0, 0) scale(1)', opacity: 0.96 },
            { transform: `translate(${targetX - startX}px, ${targetY - startY}px) scale(0.72)`, opacity: 0.2 }
        ],
        { duration: 650, easing: 'cubic-bezier(0.2, 0.8, 0.2, 1)' }
    ).onfinish = () => chip.remove();
}

/** Live log interval handle */
let liveLogInterval = null;
/** Which profile is currently shown in the live log */
let liveLogProfileId = null;
/** Last known live log content (for delta rendering) */
let liveLogLastSnapshot = '';

/** Loads running instances from the backend, detects stopped ones, and updates buttons + live log */
async function syncRunningProfiles() {
    try {
        const ids = await invoke('get_running_profiles');
        const newSet = new Set(ids);

        for (const id of runningProfiles) {
            if (!newSet.has(id)) {
                // This instance has now stopped
                runningProfiles.delete(id);
                updateAllPlayStopButtons(id);
                if (liveLogProfileId === id) {
                    const el = getLogContentElement();
                    if (el) refreshLiveLog(id, el);
                }
            }
        }
        // Add newly running profiles
        for (const id of newSet) {
            if (!runningProfiles.has(id)) {
                runningProfiles.add(id);
                updateAllPlayStopButtons(id);
            }
        }
    } catch (e) { /* silent */ }

    syncSessionWidgetWithRunningProfiles();
}

/** Starts periodic polling (every 3s) to reliably detect process exits */
function startInstancePolling() {
    setInterval(syncRunningProfiles, 3000);
}

/** Updates ALL Play/Stop buttons in the DOM for a specific profile (or all) */
function updateAllPlayStopButtons(profileId) {
    document.querySelectorAll('[data-play-btn]').forEach(btn => {
        const id = btn.dataset.playBtn;
        if (profileId && id !== profileId) return;
        const running = runningProfiles.has(id);
        if (running) {
            btn.innerHTML = '<i class="bi bi-stop-fill"></i> Stop';
            btn.style.background = '#c0392b';
            btn.style.borderColor = '#c0392b';
            btn.onclick = (e) => { e.stopPropagation(); stopProfile(id); };
        } else {
            btn.innerHTML = '<i class="bi bi-play-fill"></i> Play';
            btn.style.background = '';
            btn.style.borderColor = '';
            btn.onclick = (e) => { e.stopPropagation(); launchProfile(id); };
        }
    });
}

/** Stops a running Minecraft instance */
async function stopProfile(profileId) {
    try {
        await invoke('stop_profile', { profileId });
        runningProfiles.delete(profileId);
        updateAllPlayStopButtons(profileId);
        if (liveLogProfileId === profileId) {
            const el = getLogContentElement();
            if (el) refreshLiveLog(profileId, el);
        }
        syncSessionWidgetWithRunningProfiles();
        showToast('Instance stopped', 'info', 2000);
    } catch (e) {
        showToast('Error stopping: ' + e, 'error', 3000);
    }
}
// ────────────────────────────────────────────────────────────────────────────

// Initialize
document.addEventListener('DOMContentLoaded', async () => {
    debugLog('Lion Launcher starting...', 'info');

    // Apply i18n translations as early as possible
    applyTranslations();
    buildLanguageSelector();

    await resolveEmbeddedLogoUrl();
    initSidebarLogo();
    initSessionWidget();

    const grid = document.getElementById('profiles-grid');
    if (grid) {
        grid.innerHTML = `
            <div style="grid-column: 1 / -1; text-align: center; padding: 40px;">
                <div style="font-size: 36px; margin-bottom: 15px;"><i class="bi bi-box2-heart"></i></div>
                <p style="color: var(--gold);">Lion Launcher is initializing...</p>
            </div>
        `;
    }

    try {
        debugLog('Initializing launcher directories...', 'info');
        await invoke('initialize_launcher').catch(err => {
            debugLog('Initialize warning (non-critical): ' + err, 'error');
        });

        debugLog('Loading settings...', 'info');
        await loadSettings();

        debugLog('Loading accounts...', 'info');
        await loadAccounts();

        setupNavigation();
        setupModals();

        await loadProfiles();
        await syncRunningProfiles();
        startInstancePolling();

        debugLog('Loading Minecraft versions...', 'info');
        await loadMinecraftVersions();

        setupSearch();
        window.addEventListener('resize', scheduleProfileScrollableBoxesUpdate);

        // Load environment icons
        loadEnvironmentIcons();

        // Per Doppelklick geöffnete .lion-Datei importieren (macOS meldet sie zur Laufzeit per Event)
        try {
            await window.__TAURI__.event.listen('lion-file-opened', () => openPendingLionFile());
        } catch (_e) {
            // Event API not available
        }
        openPendingLionFile();

        debugLog('Lion Launcher ready!', 'success');
    } catch (error) {
        debugLog('Initialization error: ' + error, 'error');
        if (grid) {
            grid.innerHTML = `
                <div style="grid-column: 1 / -1; text-align: center; padding: 40px; color: var(--text-secondary);">
                    <div style="font-size: 36px; margin-bottom: 15px; color: #f44336;"><i class="bi bi-x-circle-fill"></i></div>
                    <p>Error starting: ${error}</p>
                </div>
            `;
        }
    }
});

// ==================== TOAST NOTIFICATIONS ====================
function showToast(message, type = 'info', duration = 3000) {
    const toast = document.createElement('div');
    const icon = type === 'success' ? '<i class="bi bi-check-circle-fill"></i>' : type === 'error' ? '<i class="bi bi-x-circle-fill"></i>' : type === 'warning' ? '<i class="bi bi-exclamation-triangle-fill"></i>' : '<i class="bi bi-info-circle-fill"></i>';
    const bgColor = type === 'success' ? '#4caf50' : type === 'error' ? '#f44336' : type === 'warning' ? '#ff9800' : 'var(--gold)';

    toast.style.cssText = `
        position: fixed;
        bottom: 20px;
        right: 20px;
        background: ${bgColor};
        color: white;
        padding: 15px 20px;
        border-radius: 8px;
        box-shadow: 0 4px 12px rgba(0,0,0,0.3);
        display: flex;
        align-items: center;
        gap: 10px;
        font-size: 14px;
        z-index: 10000;
        animation: slideIn 0.3s ease-out;
        max-width: 400px;
        word-wrap: break-word;
    `;

    toast.innerHTML = `
        <span style="font-size: 20px;">${icon}</span>
        <span>${message}</span>
    `;

    document.body.appendChild(toast);

    // Add animation CSS if not already present
    if (!document.getElementById('toast-animations')) {
        const style = document.createElement('style');
        style.id = 'toast-animations';
        style.textContent = `
            @keyframes slideIn {
                from {
                    transform: translateX(400px);
                    opacity: 0;
                }
                to {
                    transform: translateX(0);
                    opacity: 1;
                }
            }
            @keyframes slideOut {
                from {
                    transform: translateX(0);
                    opacity: 1;
                }
                to {
                    transform: translateX(400px);
                    opacity: 0;
                }
            }
        `;
        document.head.appendChild(style);
    }

    // Disappear after duration
    setTimeout(() => {
        toast.style.animation = 'slideOut 0.3s ease-in';
        setTimeout(() => {
            if (toast.parentElement) {
                toast.remove();
            }
        }, 300);
    }, duration);
}

// ==================== FILTER COLLAPSE/EXPAND ====================
function toggleFilterSection(sectionName) {
    const content = document.getElementById(`filter-${sectionName}-content`);
    const icon = document.getElementById(`collapse-${sectionName}`);

    if (!content || !icon) return;

    if (content.style.display === 'none') {
        content.style.display = 'block';
        icon.style.transform = 'rotate(0deg)';
    } else {
        content.style.display = 'none';
        icon.style.transform = 'rotate(-90deg)';
    }
}

// Navigation
function setupNavigation() {
    document.querySelectorAll('.nav-item[data-page]').forEach(item => {
        item.addEventListener('click', () => {
            navigateToPage(item.dataset.page);
        });
    });
    initHistoryNav();
}

/** Seitenwechsel über die Sidebar */
function navigateToPage(page) {
    if (page === 'mods') {
        recordNavigation({ type: 'browser', context: 'global' });
        showGlobalContentBrowser();
        return;
    }
    // Profile und Server tragen sich beim Laden der Übersicht selbst ein
    if (page !== 'profiles' && page !== 'servers-hub') {
        recordNavigation({ type: 'page', page });
    }
    switchPage(page);
}

// ==================== VERLAUF (Zurück / Vorwärts) ====================
// Jede Ansicht ist ein "Ort" ({ type, ... }), den die Navigationsfunktionen selbst eintragen.
// Zurück/Vorwärts stellen einen Ort über dieselben Funktionen wieder her; deren erneuter
// Eintrag trifft dann den aktuellen Ort und wird ignoriert.
const NAV_HISTORY_LIMIT = 50;
const navHistory = [];
let navHistoryIndex = -1;
/** Für welchen Kontext die Ergebnisse im Content Browser gerade geladen sind */
let browserViewKey = 'browser:global';

function navLocationKey(location) {
    switch (location?.type) {
        case 'profile': return `profile:${location.profileId}`;
        case 'server': return `server:${location.serverId}`;
        case 'browser': return `browser:${location.context}${location.profileId || location.serverId ? ':' + (location.profileId || location.serverId) : ''}`;
        case 'mod': return `mod:${location.modId}`;
        default: return `page:${location?.page}`;
    }
}

function recordNavigation(location) {
    const current = navHistory[navHistoryIndex];
    if (current && navLocationKey(current) === navLocationKey(location)) return;

    // Wie im Browser: nach "Zurück" verwirft eine neue Navigation den Vorwärts-Verlauf
    navHistory.splice(navHistoryIndex + 1);
    navHistory.push({ ...location });
    if (navHistory.length > NAV_HISTORY_LIMIT) navHistory.shift();
    navHistoryIndex = navHistory.length - 1;
    updateHistoryNavButtons();
}

/** Merkt sich Tab-Wechsel innerhalb der aktuellen Ansicht (ohne neuen Verlaufseintrag). */
function patchCurrentNavLocation(matches, patch) {
    const entry = navHistory[navHistoryIndex];
    if (entry && matches(entry)) Object.assign(entry, patch);
}

function isNavLocationAvailable(location) {
    if (location.profileId && (location.type === 'profile' || location.type === 'browser')) {
        return profiles.some(p => p.id === location.profileId);
    }
    if (location.serverId && (location.type === 'server' || location.type === 'browser')) {
        return serverProfiles.some(s => s.id === location.serverId);
    }
    return true;
}

async function navigateHistory(step) {
    let target = navHistoryIndex + step;
    // Gelöschte Profile/Server und doppelte Einträge überspringen
    while (navHistory[target] && (!isNavLocationAvailable(navHistory[target]) ||
        navLocationKey(navHistory[target]) === navLocationKey(navHistory[navHistoryIndex]))) {
        navHistory.splice(target, 1);
        if (step < 0) {
            navHistoryIndex -= 1;
            target -= 1;
        }
    }
    if (!navHistory[target]) {
        updateHistoryNavButtons();
        return;
    }

    navHistoryIndex = target;
    updateHistoryNavButtons();
    try {
        await restoreNavLocation(navHistory[target]);
    } catch (error) {
        debugLog('History navigation failed: ' + error, 'error');
    }
}

function historyBack() {
    return navigateHistory(-1);
}

function historyForward() {
    return navigateHistory(1);
}

async function restoreNavLocation(location) {
    switch (location.type) {
        case 'profile':
            skipLoadProfiles = true;
            switchPage('profiles');
            showProfileDetails(location.profileId, { mainCategory: location.mainCategory, subTab: location.subTab });
            break;
        case 'browser':
            await restoreContentBrowser(location);
            break;
        case 'mod':
            if (location.serverId) {
                if (serverBrowserContext?.serverId !== location.serverId) {
                    serverBrowserContext = await createServerBrowserContext(location.serverId);
                }
                currentProfile = null;
            } else {
                if (serverBrowserContext) {
                    serverBrowserContext = null;
                    restoreDefaultBrowserMode();
                }
                currentProfile = profiles.find(p => p.id === location.profileId) || null;
            }
            await showModDetails(location.modId, location.source || 'modrinth');
            break;
        case 'server':
            pendingServerDetailOpen = { id: location.serverId, tab: location.tab || 'console' };
            switchPage('servers-hub');
            break;
        default:
            if (location.page === 'servers-hub') pendingServerDetailOpen = null;
            switchPage(location.page);
    }
}

/** Zeigt den Content Browser wieder an – ohne Neuladen, wenn Suche/Seite noch zum Kontext gehören. */
async function restoreContentBrowser(location) {
    const sameView = browserViewKey === navLocationKey(location);

    if (location.context === 'profile') {
        const profile = profiles.find(p => p.id === location.profileId);
        if (sameView && profile) {
            currentProfile = profile;
            openedFromProfile = true;
            switchPage('mods');
        } else {
            currentProfileSubTab = location.contentType || 'mods';
            openContentBrowser(location.profileId);
        }
    } else if (location.context === 'server') {
        if (sameView && serverBrowserContext?.serverId === location.serverId) {
            serverBrowserNavigation = true;
            switchPage('mods');
        } else {
            await openServerContentBrowser(location.serverId);
        }
    } else {
        showGlobalContentBrowser(location.contentType);
    }
}

/** Content Browser ohne Profil/Server; lädt neu, falls noch Ergebnisse eines anderen Kontexts angezeigt werden. */
function showGlobalContentBrowser(contentType = null) {
    const sameView = browserViewKey === 'browser:global';
    switchPage('mods', { keepBrowserFilters: sameView });
    if (!sameView) {
        resetFiltersToDefault();
        switchContentType(contentType || currentContentType || 'mods', { force: true });
        browserViewKey = 'browser:global';
    } else if (contentType && contentType !== currentContentType) {
        switchContentType(contentType);
    }
}

function updateHistoryNavButtons() {
    const backBtn = document.getElementById('history-back-btn');
    const forwardBtn = document.getElementById('history-forward-btn');
    if (backBtn) backBtn.disabled = navHistoryIndex <= 0;
    if (forwardBtn) forwardBtn.disabled = navHistoryIndex >= navHistory.length - 1;
}

function initHistoryNav() {
    document.getElementById('history-back-btn')?.addEventListener('click', historyBack);
    document.getElementById('history-forward-btn')?.addEventListener('click', historyForward);

    // Wie im Browser: Maus-Seitentasten und Alt+←/→
    window.addEventListener('mouseup', (event) => {
        if (event.button === 3) {
            event.preventDefault();
            historyBack();
        } else if (event.button === 4) {
            event.preventDefault();
            historyForward();
        }
    });
    window.addEventListener('keydown', (event) => {
        if (!event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
        if (event.key === 'ArrowLeft') {
            event.preventDefault();
            historyBack();
        } else if (event.key === 'ArrowRight') {
            event.preventDefault();
            historyForward();
        }
    });
    updateHistoryNavButtons();
}

function switchPage(page, options = {}) {
    // Stop mods watcher when the page changes
    if (typeof stopModsWatcher === 'function') {
        stopModsWatcher();
    }

    document.querySelectorAll('.nav-item').forEach(item => {
        item.classList.remove('active');
    });
    const activeNav = document.querySelector(`.nav-item[data-page="${page}"]`);
    if (activeNav) activeNav.classList.add('active');

    document.querySelectorAll('.page').forEach(p => {
        p.classList.add('hidden');
    });
    const activePage = document.getElementById(`page-${page}`);
    if (activePage) activePage.classList.remove('hidden');

    currentPage = page;

    // When switching to the profiles page, load the overview (unless skipLoadProfiles is set)
    if (page === 'profiles') {
        if (!skipLoadProfiles) {
            loadProfiles(); // Loads the profile overview
        } else {
            skipLoadProfiles = false; // Reset for next time
        }
    }

    // Server content browser context: keep it only for navigation that belongs to it
    if (serverBrowserContext && page !== 'mod-details' && !(page === 'mods' && serverBrowserNavigation)) {
        serverBrowserContext = null;
        restoreDefaultBrowserMode();
    }

    // When switching to the content browser
    if (page === 'mods' && serverBrowserNavigation) {
        // Opened from a local server - filters are configured by applyServerBrowserMode()
        currentProfile = null;
        openedFromProfile = false;
    } else if (page === 'mods') {
        if (!openedFromProfile) {
            // Opened from main menu - clear currentProfile and reset filters
            currentProfile = null;
            // Reset filters to default ("All Loaders") – außer bei Zurück in denselben Browser
            if (!options.keepBrowserFilters) {
                setTimeout(() => {
                    resetFiltersToDefault();
                }, 100);
            }
            debugLog('Content Browser opened from main menu - filters reset to default', 'info');
        } else {
            // Opened from a profile - currentProfile is kept for auto-filter
            debugLog('Content Browser opened from profile - auto-filters will be applied', 'info');
        }
        // Reset flag for next time
        openedFromProfile = false;
    }

    // Initialize skin viewer when the skins page is opened
    if (page === 'skins') {
        setTimeout(() => {
            initSkinViewer();
            updateSkinPage();
        }, 100);
    }


    // Show/hide modpacks button depending on context
    const modpacksBtn = document.querySelector('[data-content-type="modpacks"]');
    if (modpacksBtn) {
        if (page === 'mods' && (currentProfile || serverBrowserContext)) {
            // Opened from a profile or server - hide modpacks
            modpacksBtn.style.display = 'none';
        } else if (page === 'mods') {
            // Opened normally - show modpacks
            modpacksBtn.style.display = '';
        }
    }

    // Update cache when the mod browser is opened and re-render
    if (page === 'mods') {
        if (!currentProfile) {
            debugLog('Warning: No profile selected when opening mod browser', 'warn');
        } else {
            debugLog('Opening mod browser for profile: ' + currentProfile.name, 'info');

            // Set filters automatically based on the profile
            applyProfileFilters(currentProfile);
        }

        // IMPORTANT: This code runs AFTER openContentBrowser/switchContentType!
        // So it should NOT load the content - switchContentType already does that!
        // We only load the installedModIds cache.
        loadInstalledModIds();
    }

    if (page === 'servers-hub') {
        loadServerHub();
    } else {
        clearServerHubIntervals();
    }

    serverBrowserNavigation = false;
}

// Applies the filters based on the profile
function applyProfileFilters(profile) {
    debugLog('Applying profile filters for: ' + profile.name + ', ContentType: ' + currentContentType, 'info');

    // Set Minecraft version filter (for all content types)
    const versionFilter = document.getElementById('filter-version');
    if (versionFilter && profile.minecraft_version) {
        versionFilter.value = profile.minecraft_version;
        selectedFilters.version = profile.minecraft_version;
        debugLog('Set version filter to: ' + profile.minecraft_version, 'info');
    }

    // Set mod loader filter ONLY for mods/modpacks
    if (currentContentType === 'mods' || currentContentType === 'modpacks') {
        const loaderName = profile.loader.loader;
        if (loaderName && loaderName !== 'vanilla') {
            // Set the loader dropdown
            const loaderSelect = document.getElementById('filter-loader');
            if (loaderSelect) {
                loaderSelect.value = loaderName;
                debugLog('Set loader dropdown to: ' + loaderName, 'info');
            }

            // Find and activate the button for the loader (if buttons exist)
            const loaderBtn = document.querySelector(`[data-loader="${loaderName}"]`);
            if (loaderBtn) {
                // Remove active from all loader buttons
                document.querySelectorAll('[data-loader]').forEach(b => b.classList.remove('active'));
                // Set the correct one as active
                loaderBtn.classList.add('active');
                debugLog('Set loader button to: ' + loaderName, 'info');
            }

            selectedFilters.loader = loaderName;
        } else {
            // Vanilla = no loader filter
            const loaderSelect = document.getElementById('filter-loader');
            if (loaderSelect) {
                loaderSelect.value = ''; // "All Loaders"
            }
            document.querySelectorAll('[data-loader]').forEach(b => b.classList.remove('active'));
            selectedFilters.loader = '';
        }
    }

    // Do NOT trigger a search! loadPopularContent() was already called by switchContentType()
    debugLog('Filters applied, content already loaded by switchContentType()', 'info');
}

// Resets all filters to default (for the content browser opened from the main menu)
function resetFiltersToDefault() {
    debugLog('Resetting filters to default (All Loaders)', 'info');

    // Reset Loader Filter
    const loaderSelect = document.getElementById('filter-loader');
    if (loaderSelect) {
        loaderSelect.value = ''; // "All Loaders"
    }

    // Reset Loader Buttons
    document.querySelectorAll('[data-loader]').forEach(b => b.classList.remove('active'));

    // Reset selectedFilters
    selectedFilters.loader = '';
    selectedFilters.version = '';

    // Reset Version Filter
    const versionFilter = document.getElementById('filter-version');
    if (versionFilter) {
        versionFilter.value = '';
    }

    debugLog('Filters reset complete', 'info');
}

// Profiles
async function loadProfiles() {
    recordNavigation({ type: 'page', page: 'profiles' });
    if (typeof stopModsWatcher === 'function') stopModsWatcher();
    if (typeof stopLiveLog === 'function') stopLiveLog();
    currentProfile = null;

    try {
        debugLog('Loading profiles...', 'info');
        const profileList = await invoke('get_profiles');
        debugLog('Profiles loaded: ' + (profileList.profiles?.length || 0) + ' profiles', 'success');
        profiles = profileList.profiles || [];
        renderProfiles();
    } catch (error) {
        debugLog('Failed to load profiles: ' + error, 'error');
        const grid = document.getElementById('profiles-grid');
        if (grid) {
            grid.innerHTML = `
                <div style="grid-column: 1 / -1; text-align: center; padding: 40px; color: var(--text-secondary);">
                    <div style="font-size: 36px; margin-bottom: 15px; color: #ff9800;"><i class="bi bi-exclamation-triangle-fill"></i></div>
                    <p style="margin-bottom: 10px;">Error loading profiles</p>
                    <p style="font-size: 14px; color: #888;">${error}</p>
                    <button class="btn" onclick="loadProfiles()" style="margin-top: 20px;">
                        Try again
                    </button>
                </div>
            `;
        }
    }
}

function renderProfiles() {
    const grid = document.getElementById('profiles-grid');
    if (!grid) return;

    if (profiles.length === 0) {
        // If no profiles: only show the create card
        grid.innerHTML = `
            <div class="profile-card" onclick="openCreateProfileModal()" 
                 style="cursor: pointer; background: var(--bg-light); border: 2px dashed var(--gold); display: flex; flex-direction: column; align-items: center; justify-content: center; transition: all 0.3s;"
                 onmouseover="this.style.background='var(--bg-dark)'; this.style.transform='scale(1.02)';"
                 onmouseout="this.style.background='var(--bg-light)'; this.style.transform='scale(1)';">
                <div style="font-size: 64px; color: var(--gold); margin-bottom: 15px;">+</div>
                <div style="color: var(--gold); font-weight: 600; font-size: 16px;">Create Profile</div>
            </div>
        `;
        return;
    }

    // Sort profiles by "Last played" (newest first)
    // Profiles without last_played go to the end
    const sortedProfiles = [...profiles].sort((a, b) => {
        if (!a.last_played && !b.last_played) return 0;
        if (!a.last_played) return 1;
        if (!b.last_played) return -1;
        return new Date(b.last_played) - new Date(a.last_played);
    });

    // Profile cards + create card at the end
    const profileCards = sortedProfiles.map(profile => {
        // Format the modloader name (capitalize first letter)
        const loaderName = profile.loader.loader.charAt(0).toUpperCase() + profile.loader.loader.slice(1);
        const loaderDisplay = profile.loader.loader === 'vanilla' ? 'Vanilla' : loaderName;

        // Icon: if icon_path is present (data URL), show it, otherwise the app icon
        const iconHTML = profile.icon_path
            ? `<img src="${profile.icon_path}" alt="Profile Icon" style="width: 100%; height: 100%; object-fit: cover; border-radius: 8px;" data-fallback="logo">`
            : `<img src="${LION_LOGO_URL}" alt="Lion Launcher" style="width: 100%; height: 100%; object-fit: cover; border-radius: 8px;">`;

        return `
        <div class="profile-card" data-context-menu="profile" data-profile-id="${profile.id}"
             onclick="showProfileDetails('${profile.id}')"
             oncontextmenu="showProfileContextMenu(event, '${profile.id}')">
            <div class="profile-icon" style="font-size: 48px;">
                ${iconHTML}
            </div>
            <div class="profile-name">${profile.name}</div>
            <div class="profile-info">${loaderDisplay} ${profile.minecraft_version}</div>
            <button class="btn" data-play-btn="${profile.id}"
                    onclick="event.stopPropagation(); launchProfile('${profile.id}')" 
                    style="width: 100%; margin-top: 15px; font-size: 14px; padding: 12px;"><i class="bi bi-play-fill"></i> Play</button>
        </div>
    `});

    // Add the create card at the end
    const createCard = `
        <div class="profile-card" onclick="openCreateProfileModal()" 
             style="cursor: pointer; background: var(--bg-light); border: 2px dashed var(--gold); display: flex; flex-direction: column; align-items: center; justify-content: center; transition: all 0.3s;"
             onmouseover="this.style.background='var(--bg-dark)'; this.style.transform='scale(1.02)';"
             onmouseout="this.style.background='var(--bg-light)'; this.style.transform='scale(1)';">
            <div style="font-size: 64px; color: var(--gold); margin-bottom: 15px;">+</div>
            <div style="color: var(--gold); font-weight: 600; font-size: 16px;">Create Profile</div>
        </div>
    `;

    grid.innerHTML = profileCards.join('') + createCard;
    // Buttons werden immer als "Play" gerendert -> Zustand laufender Instanzen übernehmen
    updateAllPlayStopButtons();
}

// Profile context menu on right-click
function showProfileContextMenu(event, profileId) {
    event.preventDefault();
    event.stopPropagation();

    // Remove old context menu if present
    const existingMenu = document.getElementById('profile-context-menu');
    if (existingMenu) existingMenu.remove();

    const profile = profiles.find(p => p.id === profileId);
    if (!profile) return;

    const menu = document.createElement('div');
    menu.id = 'profile-context-menu';
    menu.style.cssText = `
        position: fixed;
        top: ${event.clientY}px;
        left: ${event.clientX}px;
        background: var(--bg-dark);
        border: 2px solid var(--gold);
        border-radius: 8px;
        padding: 5px 0;
        z-index: 10000;
        min-width: 150px;
        box-shadow: 0 5px 20px rgba(0,0,0,0.5);
    `;

    menu.innerHTML = `
        <div onclick="openProfileSettings('${profileId}')" 
             style="padding: 10px 20px; cursor: pointer; color: var(--text-primary); display: flex; align-items: center; gap: 10px;"
             onmouseover="this.style.background='var(--bg-light)'" 
             onmouseout="this.style.background='transparent'">
            <i class="bi bi-gear-fill"></i> Settings
        </div>
        <div onclick="openShareProfileModal('${profileId}')" 
             style="padding: 10px 20px; cursor: pointer; color: var(--text-primary); display: flex; align-items: center; gap: 10px;"
             onmouseover="this.style.background='var(--bg-light)'" 
             onmouseout="this.style.background='transparent'">
            <i class="bi bi-share-fill"></i> Share
        </div>
        <div onclick="deleteProfile('${profileId}')" 
             style="padding: 10px 20px; cursor: pointer; color: #f44336; display: flex; align-items: center; gap: 10px;"
             onmouseover="this.style.background='var(--bg-light)'" 
             onmouseout="this.style.background='transparent'">
            <i class="bi bi-trash"></i> Delete
        </div>
    `;

    document.body.appendChild(menu);

    // Close menu on click outside
    const closeMenu = (e) => {
        if (!menu.contains(e.target)) {
            menu.remove();
            document.removeEventListener('click', closeMenu);
        }
    };
    setTimeout(() => document.addEventListener('click', closeMenu), 10);
}

function sanitizeLionName(name) {
    return String(name || 'profile')
        .trim()
        .replace(/[^a-zA-Z0-9-_ ]+/g, '')
        .replace(/\s+/g, '_')
        .slice(0, 64) || 'profile';
}

function closeDynamicModal(modalId) {
    const modal = document.getElementById(modalId);
    if (modal) modal.remove();
}

function getCheckedValues(root, selector) {
    return Array.from(root.querySelectorAll(selector))
        .filter(el => el.checked)
        .map(el => el.value);
}

async function buildLionPayload(profileId, includes, options = {}) {
    const embedMissing = options.embedMissing !== false;
    const profile = profiles.find(p => p.id === profileId);
    if (!profile) throw new Error('Profile not found');

    const includeSet = new Set(includes || []);
    const payload = {
        format: 'lion-profile',
        version: LION_SHARE_FORMAT_VERSION,
        exportedAt: new Date().toISOString(),
        profile: {
            name: profile.name,
            minecraft_version: profile.minecraft_version,
            loader: profile.loader?.loader || 'vanilla',
            loader_version: profile.loader?.version || '',
            icon_path: profile.icon_path || LION_LOGO_URL,
            memory_mb: profile.memory_mb || null,
            java_args: Array.isArray(profile.java_args) ? [...profile.java_args] : [],
            settings_sync: !!profile.settings_sync
        },
        content: {}
    };

    // Mods / Packs: exakte Versionen (per Datei-Hash über Modrinth ermittelt). Dateien, die es
    // nicht auf Modrinth gibt, werden direkt eingebettet – so fehlt beim Empfänger nichts.
    for (const kind of LION_CONTENT_KINDS) {
        if (includeSet.has(kind)) {
            payload.content[kind] = await invoke('export_profile_content', { profileId, kind, embedMissing });
        }
    }

    if (includeSet.has('servers')) {
        const servers = await invoke('get_servers', { profileId });
        payload.content.servers = (servers || []).map(s => ({
            name: s.name || s.ip,
            ip: s.ip
        }));
    }

    if (includeSet.has('worlds')) {
        const worlds = await invoke('get_worlds', { profileId });
        payload.content.worlds = (worlds || []).map(w => ({
            name: w.name,
            folder_name: w.folder_name,
            last_played: w.last_played,
            game_mode: w.game_mode,
            difficulty: w.difficulty,
            size_bytes: w.size_bytes
        }));
    }

    if (includeSet.has('logs')) {
        const latestLog = await invoke('get_profile_logs', { profileId, logType: 'latest' }).catch(() => '');
        payload.content.logs = {
            latest: latestLog ? String(latestLog).split('\n').slice(-600).join('\n') : ''
        };
    }

    const shareData = await invoke('export_profile_share_data', {
        profileId,
        includeWorldData: includeSet.has('worlds'),
        includeLogsData: includeSet.has('logs'),
        includeSettingsData: includeSet.has('settings')
    });
    payload.content.share_data = shareData || {};

    return payload;
}

function downloadLionPayload(payload, suggestedName) {
    const data = JSON.stringify(payload, null, 2);
    const blob = new Blob([data], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = sanitizeLionName(suggestedName) + '.lion';
    document.body.appendChild(anchor);
    anchor.click();
    anchor.remove();
    URL.revokeObjectURL(url);
}

async function saveLionPayloadWithDialog(payload, suggestedName) {
    const content = JSON.stringify(payload, null, 2);
    const defaultFileName = sanitizeLionName(suggestedName) + '.lion';
    return invoke('save_lion_file_with_dialog', {
        defaultFileName,
        content
    });
}

function createImportReport() {
    return {
        mods: { success: 0, failed: 0 },
        resourcepacks: { success: 0, failed: 0 },
        shaderpacks: { success: 0, failed: 0 },
        servers: { success: 0, failed: 0 },
        settingsApplied: false,
        worldsApplied: false,
        logsApplied: false
    };
}

/** Zeigt nach dem Import, welche Inhalte fehlgeschlagen sind bzw. abweichen (statt sie still zu verschlucken). */
function showImportIssues(failures, warnings) {
    if (!failures.length && !warnings.length) return;
    closeDynamicModal('import-issues-modal');

    const renderList = (items, color) => `
        <ul style="margin: 6px 0 14px 0; padding-left: 18px; color: ${color}; font-size: 12px; line-height: 1.6;">
            ${items.map(item => `<li>${escapeHtml(item)}</li>`).join('')}
        </ul>`;

    const modal = document.createElement('div');
    modal.id = 'import-issues-modal';
    modal.style.cssText = 'position: fixed; inset: 0; background: rgba(0,0,0,0.82); display: flex; align-items: center; justify-content: center; z-index: 10002;';
    modal.innerHTML = `
        <div style="background: var(--bg-dark); border: 2px solid var(--gold); border-radius: 12px; width: min(620px, 92vw); max-height: 80vh; display: flex; flex-direction: column; padding: 22px;" onclick="event.stopPropagation()">
            <h2 style="margin: 0 0 10px 0; color: var(--gold); font-size: 20px;"><i class="bi bi-exclamation-triangle"></i> Import notes</h2>
            <div style="overflow-y: auto; min-height: 0;">
                ${failures.length ? `<div style="color: var(--text-primary); font-weight: 600;">Not installed (${failures.length})</div>${renderList(failures, '#f44336')}` : ''}
                ${warnings.length ? `<div style="color: var(--text-primary); font-weight: 600;">Different version installed (${warnings.length})</div>${renderList(warnings, '#ff9800')}` : ''}
            </div>
            <div style="display: flex; justify-content: flex-end; margin-top: 12px;">
                <button class="btn" onclick="closeDynamicModal('import-issues-modal')">OK</button>
            </div>
        </div>
    `;
    modal.addEventListener('click', () => closeDynamicModal('import-issues-modal'));
    document.body.appendChild(modal);
}

function formatImportReport(report) {
    return [
        `Mods ${report.mods.success}/${report.mods.success + report.mods.failed}`,
        `RP ${report.resourcepacks.success}/${report.resourcepacks.success + report.resourcepacks.failed}`,
        `Shader ${report.shaderpacks.success}/${report.shaderpacks.success + report.shaderpacks.failed}`,
        `Server ${report.servers.success}/${report.servers.success + report.servers.failed}`,
        `Settings ${report.settingsApplied ? 'ok' : 'skip'}`,
        `Worlds ${report.worldsApplied ? 'ok' : 'skip'}`,
        `Logs ${report.logsApplied ? 'ok' : 'skip'}`
    ].join(' | ');
}

function openShareProfileModal(profileId) {
    closeDynamicModal('profile-context-menu');
    closeDynamicModal('share-profile-modal');

    const profile = profiles.find(p => p.id === profileId);
    if (!profile) return;

    const modal = document.createElement('div');
    modal.id = 'share-profile-modal';
    modal.style.cssText = 'position: fixed; inset: 0; background: rgba(0,0,0,0.82); display: flex; align-items: center; justify-content: center; z-index: 10000;';

    modal.innerHTML = `
        <div style="background: var(--bg-dark); border: 2px solid var(--gold); border-radius: 12px; width: min(520px, 92vw); padding: 22px;" onclick="event.stopPropagation()">
            <h2 style="margin: 0 0 14px 0; color: var(--gold); font-size: 20px;"><i class="bi bi-share-fill"></i> Share Profile</h2>
            <p style="margin: 0 0 14px 0; color: var(--text-secondary); font-size: 13px;">Choose what goes into the <code>.lion</code> file.</p>
            <div id="share-profile-options" style="display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px 14px; color: var(--text-primary);">
                <label><input type="checkbox" value="settings" checked> Settings (1:1)</label>
                <label><input type="checkbox" value="mods" checked> Mods (Links)</label>
                <label><input type="checkbox" value="resourcepacks" checked> Resource Packs (Links)</label>
                <label><input type="checkbox" value="shaderpacks" checked> Shader Packs (Links)</label>
                <label><input type="checkbox" value="servers" checked> Server list</label>
                <label><input type="checkbox" value="worlds"> Worlds (voll)</label>
                <label><input type="checkbox" value="logs"> Logs (voll)</label>
            </div>
            <p style="margin: 12px 0 0 0; color: var(--text-secondary); font-size: 12px;">
                Mods &amp; packs are shared in exactly your installed versions.
            </p>
            <label style="display:flex; align-items:flex-start; gap:8px; color: var(--text-secondary); margin-top: 8px; font-size: 12px;">
                <input type="checkbox" id="share-embed-missing" checked style="margin-top: 2px;">
                <span>Embed files that are not on Modrinth (e.g. CurseForge-only or own files) – otherwise they will be missing for the recipient. Makes the file larger.</span>
            </label>
            <div style="display: flex; justify-content: flex-end; gap: 10px; margin-top: 18px;">
                <button class="btn btn-secondary" onclick="closeDynamicModal('share-profile-modal')">Cancel</button>
                <button class="btn" id="share-profile-export-btn">Export .lion</button>
            </div>
        </div>
    `;

    modal.addEventListener('click', () => closeDynamicModal('share-profile-modal'));
    document.body.appendChild(modal);

    const exportBtn = document.getElementById('share-profile-export-btn');
    if (exportBtn) {
        exportBtn.addEventListener('click', async () => {
            const root = document.getElementById('share-profile-options');
            if (!root) return;
            const includes = getCheckedValues(root, 'input[type="checkbox"]');
            if (includes.length === 0) {
                showToast('Choose at least one section to share.', 'warning', 2800);
                return;
            }

            exportBtn.disabled = true;
            exportBtn.textContent = 'Export...';
            try {
                const embedMissing = !!document.getElementById('share-embed-missing')?.checked;
                const payload = await buildLionPayload(profileId, includes, { embedMissing });
                let savedPath = null;
                try {
                    savedPath = await saveLionPayloadWithDialog(payload, profile.name);
                } catch (_dialogError) {
                    // Fallback for environments without a native save dialog.
                    downloadLionPayload(payload, profile.name);
                }

                if (savedPath) {
                    showToast('Profile exported: ' + savedPath, 'success', 3200);
                    // Hinweis auf eingebettete bzw. fehlende Dateien
                    const entries = LION_CONTENT_KINDS.flatMap(kind => payload.content[kind] || []);
                    const embedded = entries.filter(e => e.data_base64);
                    const localOnly = entries.filter(e => e.source === 'local');
                    if (embedded.length) {
                        const mb = embedded.reduce((sum, e) => sum + (e.data_base64.length * 3 / 4), 0) / (1024 * 1024);
                        showToast(`${embedded.length} file(s) not on Modrinth were embedded (${mb.toFixed(1)} MB).`, 'info', 5000);
                    }
                    if (localOnly.length) {
                        showToast(`${localOnly.length} file(s) are not on Modrinth and were NOT embedded – they will be missing for the recipient.`, 'warning', 6500);
                    }
                } else {
                    showToast('Export canceled.', 'info', 2200);
                    return;
                }
                closeDynamicModal('share-profile-modal');
            } catch (error) {
                showToast('Export failed: ' + error, 'error', 4500);
            } finally {
                exportBtn.disabled = false;
                exportBtn.textContent = 'Export .lion';
            }
        });
    }
}

function openLionImportDialog() {
    const input = document.getElementById('lion-import-input');
    if (!input) return;
    input.value = '';
    input.click();
}

function closeImportLionModal() {
    closeDynamicModal('import-lion-modal');
    pendingLionImportPayload = null;
}

function buildImportSelectionFromModal() {
    const root = document.getElementById('import-lion-options');
    if (!root) return [];
    return getCheckedValues(root, 'input[type="checkbox"]');
}

/**
 * Lädt Minecraft + Loader eines Profils herunter. Mit `progressRange` ({ from, to, activity })
 * wird der Download-Fortschritt in einen übergeordneten Fortschritt (z.B. Import) eingebettet.
 */
async function prepareProfileDownloadFlow(profileId, profileName, progressRange = null) {
    const from = progressRange?.from ?? 0;
    const to = progressRange?.to ?? 100;
    const activity = progressRange?.activity ?? null;
    const scalePercent = value => from + (Math.max(0, Math.min(100, value)) / 100) * (to - from);
    let unlistenPrepareProgress = null;
    try {
        setSessionWidgetState({
            profileId,
            profileName,
            phase: progressRange ? 'progress' : 'spin',
            status: 'Preparing download...',
            percent: progressRange ? from : null,
            canStop: false,
            activity
        });

        try {
            unlistenPrepareProgress = await window.__TAURI__.event.listen('launch-progress', (event) => {
                const { status, percent } = event.payload || {};
                const progressValue = Number(percent);
                const hasPercent = Number.isFinite(progressValue) && progressValue >= 0 && progressValue <= 100;
                setSessionWidgetState({
                    profileId,
                    profileName,
                    phase: hasPercent || progressRange ? 'progress' : 'spin',
                    status: status || 'Download wird vorbereitet...',
                    percent: hasPercent ? scalePercent(progressValue) : (progressRange ? sessionWidgetState.percent : null),
                    canStop: false,
                    activity
                });
            });
        } catch (_e) {
            // Event API might not be available
        }

        await invoke('prepare_profile_download', { profileId });

        if (typeof unlistenPrepareProgress === 'function') unlistenPrepareProgress();
        setSessionWidgetState({
            profileId,
            profileName,
            phase: 'idle',
            status: 'Download complete - ready to launch.',
            percent: null,
            canStop: false,
            activity: null
        });
    } catch (prepareError) {
        if (typeof unlistenPrepareProgress === 'function') unlistenPrepareProgress();
        setSessionWidgetState({
            profileId,
            profileName,
            phase: 'idle',
            status: 'Download failed: ' + prepareError,
            percent: null,
            canStop: false,
            activity: null
        });
        throw prepareError;
    }
}

/** Lädt die Profilliste neu, ohne eine geöffnete Profil-Detailansicht zu verlassen. */
async function refreshProfilesInBackground() {
    try {
        const profileList = await invoke('get_profiles');
        profiles = profileList.profiles || [];
        if (currentPage === 'profiles' && !currentProfile) renderProfiles();
    } catch (error) {
        debugLog('Failed to refresh profiles: ' + error, 'error');
    }
}

/** Legt das Zielprofil eines .lion-Imports an und liefert es zurück. */
async function createProfileForImport(name, minecraftVersion, loader, loaderVersion) {
    const knownProfileIds = new Set(profiles.map(p => p.id));
    const profileList = await invoke('create_profile', { name, minecraftVersion, loader, loaderVersion });
    profiles = profileList.profiles || [];
    if (currentPage === 'profiles' && !currentProfile) renderProfiles();

    const createdProfile = profiles.find(p => !knownProfileIds.has(p.id)) ||
        profiles.find(p => p.name === name && p.minecraft_version === minecraftVersion);
    if (!createdProfile) throw new Error('Imported profile was not created');
    return createdProfile;
}

async function runLionImport() {
    if (!pendingLionImportPayload) return;
    const payload = pendingLionImportPayload;
    const selections = buildImportSelectionFromModal();
    const fastImport = !!document.getElementById('import-lion-fast-mode')?.checked;
    if (!selections.length) {
        showToast('Choose at least one import section.', 'warning', 2800);
        return;
    }

    // Der Import läuft im Hintergrund weiter, der Fortschritt steht im Aktivitäts-Kasten oben rechts
    closeImportLionModal();
    const createModal = document.getElementById('create-profile-modal');
    if (createModal) createModal.classList.remove('active');

    const profileData = payload.profile || {};
    const profileName = String(profileData.name || 'Imported Profile').slice(0, 60);
    let importedProfileId = null;
    const setImportProgress = (percent, status) => setSessionWidgetState({
        profileId: importedProfileId,
        profileName,
        phase: 'progress',
        status,
        percent,
        canStop: false,
        activity: 'import'
    });

    setImportProgress(0, 'Importing profile...');
    animateSessionFlyIn(profileName);

    try {
        const mcVersion = profileData.minecraft_version || 'latest';
        const loader = profileData.loader || 'fabric';
        const loaderVersion = profileData.loader_version || 'latest';
        const importedIcon = profileData.icon_path || LION_LOGO_URL;

        const createdProfile = await createProfileForImport(profileName, mcVersion, loader, loaderVersion);
        importedProfileId = createdProfile.id;

        const content = payload.content || {};

        const report = createImportReport();

        setImportProgress(3, 'Applying settings...');
        if (selections.includes('settings')) {
            await invoke('update_profile', {
                profileId: createdProfile.id,
                updates: {
                    name: profileName,
                    minecraft_version: mcVersion,
                    loader,
                    loader_version: loaderVersion,
                    memory_mb: profileData.memory_mb || 4096,
                    java_args: Array.isArray(profileData.java_args) ? profileData.java_args : [],
                    icon_path: importedIcon
                }
            });
            report.settingsApplied = true;
        }

        if (selections.includes('settings') && typeof profileData.settings_sync === 'boolean') {
            await invoke('toggle_settings_sync', { profileId: createdProfile.id, enabled: !!profileData.settings_sync });
        }

        setImportProgress(6, selections.includes('worlds') ? 'Importing worlds...' : 'Importing profile data...');
        const shareData = content.share_data || {};
        await invoke('import_profile_share_data', {
            profileId: createdProfile.id,
            data: shareData,
            importWorldData: selections.includes('worlds'),
            importLogsData: selections.includes('logs'),
            importSettingsData: selections.includes('settings')
        });
        report.worldsApplied = selections.includes('worlds') && !!shareData?.worlds_archive_base64;
        report.logsApplied = selections.includes('logs') && !!shareData?.logs_archive_base64;

        // Inhalte, die importiert werden (alle Einträge – auch die ohne Modrinth-ID, damit
        // Fehlschläge gemeldet statt still übersprungen werden)
        const contentKinds = LION_CONTENT_KINDS.filter(kind =>
            selections.includes(kind) && Array.isArray(content[kind]) && content[kind].length > 0);
        const totalItems = contentKinds.reduce((sum, kind) => sum + content[kind].length, 0);

        if (selections.includes('servers') && Array.isArray(content.servers)) {
            setImportProgress(10, 'Adding servers...');
            for (const server of content.servers) {
                if (!server?.ip) continue;
                try {
                    await invoke('add_server', {
                        profileId: createdProfile.id,
                        name: server.name || server.ip,
                        ip: server.ip
                    });
                    report.servers.success += 1;
                } catch (_e) {
                    report.servers.failed += 1;
                }
            }
        }

        // Gesamtfortschritt: 0-12% Profil/Daten, 12-65% Inhalte, Rest Minecraft-Download
        const contentStart = 12;
        const contentEnd = totalItems > 0 ? 65 : contentStart;
        const importFailures = [];
        const importWarnings = [];
        let completedBefore = 0;
        let activeKind = null;
        const reportContentProgress = (kind, doneInKind) => {
            const overall = completedBefore + doneInKind;
            const pct = contentStart + (overall / Math.max(totalItems, 1)) * (contentEnd - contentStart);
            setImportProgress(pct, `Installing ${LION_CONTENT_LABELS[kind] || 'content'}... (${overall}/${totalItems})`);
        };

        let unlistenContentProgress = null;
        try {
            unlistenContentProgress = await window.__TAURI__.event.listen('content-import-progress', (event) => {
                const { kind, done } = event.payload || {};
                if (kind === activeKind) reportContentProgress(kind, Number(done) || 0);
            });
        } catch (_e) {
            // Event API might not be available
        }

        try {
            for (const kind of contentKinds) {
                activeKind = kind;
                const entries = content[kind];
                reportContentProgress(kind, 0);
                try {
                    // Backend lädt exakt die exportierten Versionen (Hash-geprüft) bzw. die
                    // eingebetteten Dateien, mit begrenzter Parallelität
                    const result = await invoke('import_profile_content', {
                        profileId: createdProfile.id,
                        kind,
                        entries,
                        parallel: fastImport
                    });
                    report[kind].success += result.installed || 0;
                    report[kind].failed += (result.failed || []).length;
                    (result.failed || []).forEach(f => importFailures.push(`${f.name}: ${f.reason}`));
                    importWarnings.push(...(result.warnings || []));
                } catch (error) {
                    report[kind].failed += entries.length;
                    importFailures.push(`${LION_CONTENT_LABELS[kind]}: ${error}`);
                }
                completedBefore += entries.length;
            }
        } finally {
            activeKind = null;
            if (typeof unlistenContentProgress === 'function') unlistenContentProgress();
        }

        await prepareProfileDownloadFlow(createdProfile.id, createdProfile.name, {
            from: contentEnd,
            to: 100,
            activity: 'import'
        });
        await refreshProfilesInBackground();
        showToast('Import complete. ' + formatImportReport(report),
            importFailures.length ? 'warning' : 'success', 6000);
        showImportIssues(importFailures, importWarnings);

    } catch (error) {
        setSessionWidgetState({ phase: 'idle', percent: null, canStop: false, activity: null });
        syncSessionWidgetWithRunningProfiles();
        await refreshProfilesInBackground();
        showToast('Import failed: ' + error, 'error', 5000);
    }
}

function openLionImportModal(payload) {
    closeImportLionModal();
    pendingLionImportPayload = payload;

    const has = (key) => {
        const c = payload?.content || {};
        return Array.isArray(c[key]) && c[key].length > 0;
    };
    const hasShare = (key) => !!payload?.content?.share_data?.[key];
    const incomingVersion = Number(payload?.version || 1);
    const isNewerFormat = Number.isFinite(incomingVersion) && incomingVersion > LION_SHARE_FORMAT_VERSION;
    const isOlderFormat = Number.isFinite(incomingVersion) && incomingVersion < LION_SHARE_FORMAT_VERSION;
    const contentSummary = LION_CONTENT_KINDS.map(key => {
        const list = Array.isArray(payload?.content?.[key]) ? payload.content[key] : [];
        if (!list.length) return null;
        const embedded = list.filter(entry => entry && entry.data_base64).length;
        const missing = list.filter(entry => entry && entry.source === 'local').length;
        const notes = [
            embedded ? `${embedded} embedded in this file` : '',
            missing ? `${missing} not included` : ''
        ].filter(Boolean).join(', ');
        return `${list.length} ${LION_CONTENT_LABELS[key]}${notes ? ` (${notes})` : ''}`;
    }).filter(Boolean).join(' · ');

    const modal = document.createElement('div');
    modal.id = 'import-lion-modal';
    modal.style.cssText = 'position: fixed; inset: 0; background: rgba(0,0,0,0.82); display: flex; align-items: center; justify-content: center; z-index: 10001;';
    modal.innerHTML = `
        <div style="background: var(--bg-dark); border: 2px solid var(--gold); border-radius: 12px; width: min(560px, 92vw); padding: 22px;" onclick="event.stopPropagation()">
            <h2 style="margin: 0 0 10px 0; color: var(--gold); font-size: 20px;"><i class="bi bi-box-arrow-in-down"></i> Import .lion</h2>
            <p style="margin: 0 0 12px 0; color: var(--text-secondary); font-size: 13px;">${escapeHtml(payload?.profile?.name || 'Imported Profile')} - choose the sections to import.</p>
            <p style="margin: 0 0 12px 0; color: ${isNewerFormat ? '#ff9800' : 'var(--text-secondary)'}; font-size: 12px;">Format v${incomingVersion} (lokal: v${LION_SHARE_FORMAT_VERSION})${isOlderFormat ? ' - older format without exact versions: the latest compatible versions will be installed.' : ''}${isNewerFormat ? ' - newer than this launcher version.' : ''}</p>
            ${contentSummary ? `<p style="margin: 0 0 12px 0; color: var(--text-secondary); font-size: 12px;">${escapeHtml(contentSummary)}</p>` : ''}
            <div id="import-lion-options" style="display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px 14px; color: var(--text-primary);">
                <label><input type="checkbox" value="settings" ${payload?.content?.share_data?.settings ? 'checked' : ''}> Settings (1:1)</label>
                <label><input type="checkbox" value="mods" ${has('mods') ? 'checked' : ''}> Mods</label>
                <label><input type="checkbox" value="resourcepacks" ${has('resourcepacks') ? 'checked' : ''}> Resource Packs</label>
                <label><input type="checkbox" value="shaderpacks" ${has('shaderpacks') ? 'checked' : ''}> Shader Packs</label>
                <label><input type="checkbox" value="servers" ${has('servers') ? 'checked' : ''}> Server list</label>
                <label><input type="checkbox" value="worlds" ${(has('worlds') || hasShare('worlds_archive_base64')) ? 'checked' : ''}> Worlds</label>
                <label><input type="checkbox" value="logs" ${(payload?.content?.logs || hasShare('logs_archive_base64')) ? 'checked' : ''}> Logs</label>
            </div>
            <label style="display:flex; align-items:center; gap:8px; color: var(--text-secondary); margin-top: 12px; font-size: 12px;">
                <input type="checkbox" id="import-lion-fast-mode"> Fast Import (parallel)
            </label>
            <div style="display: flex; justify-content: flex-end; gap: 10px; margin-top: 18px;">
                <button class="btn btn-secondary" onclick="closeImportLionModal()">Cancel</button>
                <button class="btn" id="import-lion-run-btn" ${isNewerFormat ? 'disabled' : ''}>Start Import</button>
            </div>
        </div>
    `;
    modal.addEventListener('click', closeImportLionModal);
    document.body.appendChild(modal);

    const runBtn = document.getElementById('import-lion-run-btn');
    if (runBtn) runBtn.addEventListener('click', runLionImport);
}

async function handleLionImportInputChange(event) {
    const file = event?.target?.files?.[0];
    if (!file) return;
    openLionImportFromText(await file.text().catch(() => ''));
}

/** Öffnet eine per Doppelklick / "Öffnen mit" übergebene .lion-Datei im Import-Dialog. */
async function openPendingLionFile() {
    try {
        const text = await invoke('take_pending_lion_file');
        if (text) openLionImportFromText(text);
    } catch (error) {
        showToast('Could not read import file: ' + error, 'error', 4500);
    }
}

/** Liest und prüft den Inhalt einer .lion-Datei. Wirft bei ungültigen Dateien. */
function parseLionPayload(text) {
    const payload = JSON.parse(text);
    if (payload?.format !== 'lion-profile') {
        throw new Error('Invalid .lion format');
    }
    const fileVersion = Number(payload?.version || 1);
    if (!Number.isFinite(fileVersion) || fileVersion <= 0) {
        throw new Error('Invalid .lion version number');
    }
    if (fileVersion > LION_SHARE_FORMAT_VERSION) {
        throw new Error('This .lion file is newer than your launcher (v' + fileVersion + ')');
    }
    return payload;
}

function openLionImportFromText(text) {
    let payload;
    try {
        payload = parseLionPayload(text);
    } catch (error) {
        showToast('Could not read import file: ' + error, 'error', 4500);
        return;
    }
    if (Number(payload.version || 1) < LION_SHARE_FORMAT_VERSION) {
        showToast('Older .lion version detected. Importing with fallback.', 'info', 3200);
    }
    openLionImportModal(payload);
}

// RAM slider helper functions
function updateMemoryDisplay(value) {
    const display = document.getElementById('memory-value-display');
    if (display) {
        display.textContent = `${value} MB`;
    }

    // Update progress bar
    const progressBar = document.getElementById('memory-slider-progress');
    const slider = document.getElementById('edit-profile-memory');

    if (progressBar && slider) {
        const min = parseFloat(slider.min);
        const max = parseFloat(slider.max);
        const percentage = ((value - min) / (max - min)) * 100;
        progressBar.style.width = `${percentage}%`;
    }
}

async function initMemorySlider(currentMemory) {
    try {
        // Get system RAM
        const systemMemoryMB = await invoke('get_system_memory');
        console.log('[Memory Slider] System RAM:', systemMemoryMB, 'MB');

        const slider = document.getElementById('edit-profile-memory');
        const maxLabel = document.getElementById('max-memory-label');

        if (slider && systemMemoryMB) {
            // Set maximum to 90% of system RAM (sensible upper limit)
            const maxMemory = Math.floor(systemMemoryMB * 0.9);
            slider.max = maxMemory;

            if (maxLabel) {
                maxLabel.textContent = `${Math.floor(maxMemory / 1024)} GB`;
            }

            // Set current value
            slider.value = currentMemory || 4096;

            // Initialize progress bar
            updateMemoryDisplay(slider.value);

            console.log('[Memory Slider] Initialized - Max:', maxMemory, 'MB, Current:', slider.value, 'MB');
        }
    } catch (error) {
        console.error('[Memory Slider] Failed to get system memory:', error);
        // Fallback: keep the default maximum and initialize anyway
        const slider = document.getElementById('edit-profile-memory');
        if (slider) {
            updateMemoryDisplay(slider.value);
        }
    }
}

// Open profile settings as its own modal window
async function openProfileSettings(profileId) {
    const menu = document.getElementById('profile-context-menu');
    if (menu) menu.remove();

    const profile = profiles.find(p => p.id === profileId);
    if (!profile) return;

    // Edit modal uses currentProfile as reference for preselection/state
    currentProfile = profile;

    // Remove old modal if present
    const existingModal = document.getElementById('profile-settings-modal');
    if (existingModal) existingModal.remove();

    const modal = document.createElement('div');
    modal.id = 'profile-settings-modal';
    modal.style.cssText = `
        position: fixed;
        top: 0;
        left: 0;
        right: 0;
        bottom: 0;
        background: rgba(0,0,0,0.85);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 10000;
    `;

    modal.innerHTML = `
        <div style="background: var(--bg-dark); border: 2px solid var(--gold); border-radius: 12px; 
                    width: 90%; max-width: 550px; max-height: 85vh; overflow: hidden; display: flex; flex-direction: column;">
            
            <!-- Header -->
            <div style="display: flex; justify-content: space-between; align-items: center; padding: 20px; border-bottom: 1px solid var(--bg-light);">
                <h2 style="color: var(--gold); margin: 0; font-size: 18px;"><i class="bi bi-gear-fill"></i> Profile Settings</h2>
                <button onclick="closeProfileSettingsModal('${profile.id}')" 
                        style="background: none; border: none; color: var(--text-secondary); font-size: 24px; cursor: pointer; padding: 0; line-height: 1;">
                    <i class="bi bi-x"></i>
                </button>
            </div>
            
            <!-- Scrollable content -->
            <div style="padding: 20px; overflow-y: auto; flex: 1;">
                
                <!-- Profile picture -->
                <div style="display: flex; gap: 15px; align-items: center; background: var(--bg-light); padding: 15px; border-radius: 8px; margin-bottom: 20px;">
                    <div id="profile-icon-preview" style="width: 60px; height: 60px; background: var(--bg-medium); border-radius: 8px; display: flex; align-items: center; justify-content: center; font-size: 30px; overflow: hidden;">
                        ${profile.icon_path ? `<img src="${profile.icon_path}" alt="Profile Icon" style="width: 100%; height: 100%; object-fit: cover;" data-fallback="logo">` : `<img src="${LION_LOGO_URL}" alt="Lion Launcher" style="width: 100%; height: 100%; object-fit: cover;">`}
                    </div>
                    <div style="flex: 1;">
                        <input type="file" id="profile-icon-input" accept="image/*" onchange="previewProfileIcon(event)" style="display: none;">
                        <button class="btn btn-secondary" onclick="document.getElementById('profile-icon-input').click()" style="padding: 6px 12px; font-size: 12px;">
                            <i class="bi bi-camera"></i> Image
                        </button>
                        <button class="btn btn-secondary" onclick="clearProfileIcon()" style="padding: 6px 12px; font-size: 12px; margin-left: 5px;">
                            <i class="bi bi-x"></i>
                        </button>
                    </div>
                </div>
                
                <!-- Profile name -->
                <div style="margin-bottom: 15px;">
                    <label style="display: block; margin-bottom: 5px; color: var(--text-secondary); font-size: 13px;">Profile Name</label>
                    <input type="text" value="${profile.name}" id="edit-profile-name"
                           style="width: 100%; padding: 10px; background: var(--bg-light); border: none; border-radius: 6px; color: var(--text-primary); font-size: 14px;">
                </div>
                
                <!-- Minecraft Version -->
                <div style="margin-bottom: 15px;">
                    <label style="display: block; margin-bottom: 5px; color: var(--text-secondary); font-size: 13px;">Minecraft Version</label>
                    <div style="display: flex; gap: 10px; align-items: center;">
                        <select id="edit-profile-mc-version" onchange="updateEditLoaderVersions()"
                                style="flex: 1; padding: 10px; background: var(--bg-light); border: none; border-radius: 6px; color: var(--text-primary); font-size: 14px;">
                            <option value="${profile.minecraft_version}" selected>${profile.minecraft_version}</option>
                        </select>
                        <label style="display: flex; align-items: center; gap: 5px; color: var(--text-secondary); font-size: 11px; white-space: nowrap;">
                            <input type="checkbox" id="edit-show-snapshots" onchange="updateEditVersionList()">
                            Snapshots
                        </label>
                    </div>
                </div>
                
                <!-- Mod Loader -->
                <div style="margin-bottom: 15px;">
                    <label style="display: block; margin-bottom: 5px; color: var(--text-secondary); font-size: 13px;">Mod Loader</label>
                    <div style="display: flex; gap: 8px;">
                        <select id="edit-profile-loader" onchange="updateEditLoaderVersions()"
                                style="flex: 1; padding: 10px; background: var(--bg-light); border: none; border-radius: 6px; color: var(--text-primary); font-size: 14px;">
                            <option value="vanilla" ${profile.loader.loader === 'vanilla' ? 'selected' : ''}>Vanilla</option>
                            <option value="fabric" ${profile.loader.loader === 'fabric' ? 'selected' : ''}>Fabric</option>
                            <option value="forge" ${profile.loader.loader === 'forge' ? 'selected' : ''}>Forge</option>
                            <option value="neoforge" ${profile.loader.loader === 'neoforge' ? 'selected' : ''}>NeoForge</option>
                            <option value="quilt" ${profile.loader.loader === 'quilt' ? 'selected' : ''}>Quilt</option>
                        </select>
                        <select id="edit-profile-loader-version"
                                style="flex: 1; padding: 10px; background: var(--bg-light); border: none; border-radius: 6px; color: var(--text-primary); font-size: 14px;">
                            <option value="${profile.loader.version}" selected>${profile.loader.version || 'Neueste'}</option>
                        </select>
                    </div>
                </div>
                
                <!-- Memory -->
                <div style="margin-bottom: 15px;">
                    <label style="display: block; margin-bottom: 8px; color: var(--text-secondary); font-size: 13px;">
                        RAM Allocation: <span id="memory-value-display" style="color: var(--gold); font-weight: bold;">${profile.memory_mb || 4096} MB</span>
                    </label>
                    <div style="width: 100%; position: relative; height: 20px; margin: 0 10px;">
                        <!-- Progress bar below the slider -->
                        <div style="position: absolute; width: calc(100% - 20px); height: 8px; background: var(--bg-light); border-radius: 4px; top: 6px; left: 0; right: 0; pointer-events: none;">
                            <div id="memory-slider-progress" style="height: 100%; background: var(--gold); border-radius: 4px; width: 0; transition: width 0.1s ease;"></div>
                        </div>
                        <!-- Slider -->
                        <input type="range" 
                               id="edit-profile-memory" 
                               min="512" 
                               max="16384" 
                               step="512"
                               value="${profile.memory_mb || 4096}"
                               oninput="updateMemoryDisplay(this.value)"
                               style="width: calc(100% - 20px); cursor: pointer; -webkit-appearance: none; appearance: none; background: transparent; outline: none; position: absolute; top: 0; left: 0; z-index: 2;">
                    </div>
                    <div style="display: flex; justify-content: space-between; margin-top: 12px; padding: 0 10px; font-size: 11px; color: var(--text-secondary);">
                        <span>512 MB</span>
                        <span id="max-memory-label">16 GB</span>
                    </div>
                </div>
                
                <style>
                    /* Slider Thumb */
                    #edit-profile-memory::-webkit-slider-thumb {
                        -webkit-appearance: none;
                        appearance: none;
                        width: 20px;
                        height: 20px;
                        background: var(--gold);
                        cursor: pointer;
                        border-radius: 50%;
                        border: 3px solid var(--bg-dark);
                        box-shadow: 0 2px 6px rgba(0,0,0,0.4);
                        transition: all 0.2s;
                        position: relative;
                        z-index: 3;
                    }
                    
                    #edit-profile-memory::-webkit-slider-thumb:hover {
                        transform: scale(1.15);
                        box-shadow: 0 3px 8px rgba(0,0,0,0.5);
                    }
                    
                    #edit-profile-memory::-moz-range-thumb {
                        width: 14px;
                        height: 14px;
                        background: var(--gold);
                        cursor: pointer;
                        border-radius: 50%;
                        border: 3px solid var(--bg-dark);
                        box-shadow: 0 2px 6px rgba(0,0,0,0.4);
                        transition: all 0.2s;
                    }
                    
                    #edit-profile-memory::-moz-range-thumb:hover {
                        transform: scale(1.15);
                        box-shadow: 0 3px 8px rgba(0,0,0,0.5);
                    }
                    
                    /* Track (invisible/transparent) */
                    #edit-profile-memory::-webkit-slider-runnable-track {
                        width: 100%;
                        height: 20px;
                        background: transparent;
                        border: none;
                    }
                    
                    #edit-profile-memory::-moz-range-track {
                        width: 100%;
                        height: 20px;
                        background: transparent;
                        border: none;
                    }
                </style>
                
                <!-- Java Arguments -->
                <div style="margin-bottom: 15px;">
                    <label style="display: block; margin-bottom: 5px; color: var(--text-secondary); font-size: 13px;">Java Arguments</label>
                    <textarea id="edit-profile-java-args" rows="2"
                              style="width: 100%; padding: 10px; background: var(--bg-light); border: none; border-radius: 6px; color: var(--text-primary); font-family: monospace; font-size: 12px; resize: vertical;"
                              placeholder="-XX:+UseG1GC">${(profile.java_args || []).join(' ')}</textarea>
                </div>
                
                <!-- Game Directory -->
                <div style="margin-bottom: 15px;">
                    <label style="display: block; margin-bottom: 5px; color: var(--text-secondary); font-size: 13px;">Game Directory</label>
                    <div style="display: flex; gap: 8px;">
                        <input type="text" value="${profile.game_dir}" readonly
                               style="flex: 1; padding: 10px; background: var(--bg-light); border: none; border-radius: 6px; color: var(--text-secondary); font-size: 12px;">
                        <button class="btn btn-secondary" onclick="openProfileFolder('${profile.id}')" style="padding: 10px 15px;">
                            <i class="bi bi-folder"></i>
                        </button>
                    </div>
                </div>
                
                <!-- Settings Sync -->
                <div style="margin-bottom: 15px; background: var(--bg-light); padding: 15px; border-radius: 8px;">
                    <div style="display: flex; justify-content: space-between; align-items: center;">
                        <div style="flex: 1;">
                            <label style="display: block; color: var(--text-primary); font-size: 14px; font-weight: 500;"><i class="bi bi-arrow-clockwise"></i> Sync Settings</label>
                            <span style="color: var(--text-secondary); font-size: 11px; display: block; margin-top: 5px;">
                                Automatically syncs keybinds and settings between all profiles.
                                The most recent change takes priority.
                            </span>
                        </div>
                        <label class="switch">
                            <input type="checkbox" id="edit-settings-sync" ${profile.settings_sync !== false ? 'checked' : ''} 
                                   onchange="toggleSettingsSync('${profile.id}', this.checked)">
                            <span class="settings-sync-slider"></span>
                        </label>
                    </div>
                </div>
                
                <!-- Maintenance / Repair -->
                <div style="margin-bottom: 15px; background: rgba(255, 152, 0, 0.1); border: 1px solid #ff9800; padding: 15px; border-radius: 8px;">
                    <label style="display: block; color: #ff9800; font-size: 14px; font-weight: 500; margin-bottom: 10px;"><i class="bi bi-tools"></i> Maintenance</label>
                    <div style="display: flex; gap: 10px; flex-wrap: wrap;">
                        <button class="btn btn-secondary" onclick="repairProfile('${profile.id}')" 
                                style="padding: 8px 15px; font-size: 12px; display: flex; align-items: center; gap: 6px;">
                            <i class="bi bi-arrow-clockwise"></i> Repair Installation
                        </button>
                        <button class="btn btn-secondary" onclick="clearProfileCache('${profile.id}')" 
                                style="padding: 8px 15px; font-size: 12px; display: flex; align-items: center; gap: 6px;">
                            <i class="bi bi-trash"></i> Clear Cache
                        </button>
                    </div>
                    <span style="color: var(--text-secondary); font-size: 10px; display: block; margin-top: 8px;">
                        Re-downloads Minecraft, the loader, and libraries. Mods are kept.
                    </span>
                </div>
            </div>
        </div>
    `;

    document.body.appendChild(modal);

    // Initialize RAM slider with system RAM
    initMemorySlider(profile.memory_mb || 4096);

    // Close on background click (with auto-save)
    modal.addEventListener('click', (e) => {
        if (e.target === modal) closeProfileSettingsModal(profile.id);
    });

    // Escape key closes (with auto-save)
    const escHandler = (e) => {
        if (e.key === 'Escape') {
            closeProfileSettingsModal(profile.id);
            document.removeEventListener('keydown', escHandler);
        }
    };
    document.addEventListener('keydown', escHandler);

    // Load versions (MC + loader) so more than just "Latest" is visible
    setTimeout(async () => {
        await populateEditVersionSelect();
        await updateEditLoaderVersions();
    }, 50);
}

async function closeProfileSettingsModal(profileId) {
    // Auto-save: save changes before closing
    if (profileId) {
        await saveProfileSettingsFromModal(profileId, true); // true = silent mode
    }

    const modal = document.getElementById('profile-settings-modal');
    if (modal) modal.remove();
    selectedProfileIcon = null;
}

// ==================== PROFILE REPAIR ====================

async function repairProfile(profileId) {
    const profile = profiles.find(p => p.id === profileId);
    if (!profile) {
        showToast('Profile not found', 'error', 3000);
        return;
    }

    // Confirmation dialog
    const confirmed = await showConfirmModal({
        title: 'Repair installation',
        message: 'Repair ',
        highlight: `${profile.name} (${profile.minecraft_version}, ${profile.loader.loader})`,
        suffix: '?',
        details: 'This will re-download Minecraft and all loader files. Your mods, worlds, and settings will be kept.',
        confirmText: 'Repair',
        icon: 'bi-wrench',
        danger: false
    });

    if (!confirmed) return;

    closeProfileSettingsModal();
    showToast('Starting repair...', 'info', 3000);
    debugLog('Starting repair for profile: ' + profileId, 'info');

    try {
        await invoke('repair_profile', { profileId: profileId });
        showToast('Profile repaired successfully!', 'success', 4000);
        debugLog('Profile repair completed: ' + profileId, 'success');
    } catch (error) {
        debugLog('Failed to repair profile: ' + error, 'error');
        showToast('Repair failed: ' + error, 'error', 5000);
    }
}

async function clearProfileCache(profileId) {
    const profile = profiles.find(p => p.id === profileId);
    if (!profile) {
        showToast('Profile not found', 'error', 3000);
        return;
    }

    // Confirmation dialog
    const confirmed = await showConfirmModal({
        title: 'Clear cache',
        message: 'Clear the cache of ',
        highlight: profile.name,
        suffix: '?',
        details: 'This deletes temporary files and the shader cache. Your mods, worlds, and settings will be kept.',
        confirmText: 'Clear cache',
        icon: 'bi-eraser',
        danger: false
    });

    if (!confirmed) return;

    showToast('Clearing cache...', 'info', 2000);
    debugLog('Clearing cache for profile: ' + profileId, 'info');

    try {
        await invoke('clear_profile_cache', { profileId: profileId });
        showToast('Cache cleared!', 'success', 3000);
        debugLog('Cache cleared for profile: ' + profileId, 'success');
    } catch (error) {
        debugLog('Failed to clear cache: ' + error, 'error');
        showToast('Fehler: ' + error, 'error', 4000);
    }
}

// ==================== SETTINGS SYNC ====================

async function toggleSettingsSync(profileId, enabled) {
    try {
        await invoke('toggle_settings_sync', { profileId, enabled });

        // Update slider color
        const slider = document.querySelector('.settings-sync-slider');
        if (slider) {
            slider.style.backgroundColor = enabled ? 'var(--gold)' : 'var(--bg-medium)';
        }

        showToast(enabled ? 'Settings sync enabled' : 'Settings sync disabled', 'success', 2000);
        debugLog(`Settings sync ${enabled ? 'enabled' : 'disabled'} for profile ${profileId}`);
    } catch (error) {
        debugLog('Failed to toggle settings sync: ' + error, 'error');
        showToast('Fehler: ' + error, 'error', 3000);
    }
}

async function syncSettingsFromProfile(profileId) {
    try {
        await invoke('sync_settings_from_profile', { profileId });
        showToast('Settings synced to all profiles with sync enabled!', 'success', 3000);
        debugLog('Settings synced from profile ' + profileId + ' to all other profiles');
    } catch (error) {
        debugLog('Failed to sync settings from profile: ' + error, 'error');
        showToast('Fehler: ' + error, 'error', 3000);
    }
}

async function syncSettingsToProfile(profileId) {
    try {
        await invoke('sync_settings_to_profile', { profileId });
        showToast('Default settings applied to profile', 'success', 3000);
        debugLog('Settings synced to profile ' + profileId);
    } catch (error) {
        debugLog('Failed to sync settings to profile: ' + error, 'error');
        showToast('Fehler: ' + error, 'error', 3000);
    }
}

async function saveProfileSettingsFromModal(profileId, silent = false) {
    const nameInput = document.getElementById('edit-profile-name');
    const memoryInput = document.getElementById('edit-profile-memory');
    const mcVersionSelect = document.getElementById('edit-profile-mc-version');
    const loaderSelect = document.getElementById('edit-profile-loader');
    const loaderVersionSelect = document.getElementById('edit-profile-loader-version');
    const javaArgsTextarea = document.getElementById('edit-profile-java-args');

    if (!nameInput || !memoryInput) {
        if (!silent) {
            showToast('Error: form not found', 'error', 3000);
        }
        return;
    }

    try {
        const selectedMcVersion = mcVersionSelect?.value || currentProfile?.minecraft_version;
        const selectedLoader = loaderSelect?.value || currentProfile?.loader?.loader;
        const selectedLoaderVersion = loaderVersionSelect?.value || currentProfile?.loader?.version || '';

        const updates = {
            name: nameInput.value,
            minecraft_version: selectedMcVersion,
            loader: selectedLoader,
            loader_version: selectedLoaderVersion,
            memory_mb: parseInt(memoryInput.value) || 4096,
            java_args: javaArgsTextarea?.value.split(' ').filter(a => a.trim()) || [],
            icon_path: selectedProfileIcon || null
        };

        await invoke('update_profile', {
            profileId: profileId,
            updates: updates
        });

        if (!silent) {
            showToast('Profile settings saved!', 'success', 3000);
        }
        selectedProfileIcon = null;

        // Reload profiles
        await loadProfiles();
    } catch (error) {
        debugLog('Failed to save settings: ' + error, 'error');
        if (!silent) {
            showToast('Error saving: ' + error, 'error', 5000);
        }
    }
}

async function launchProfile(profileId) {
    const profile = profiles.find(p => p.id === profileId);
    const profileName = profile ? profile.name : 'Unknown';
    let unlistenProgress = null;

    if (!currentUsername || currentUsername === 'Guest') {
        showToast('Please set your username in Settings first!', 'warning', 4000);
        navigateToPage('settings');
        return;
    }

    debugLog('Launching: ' + profileName, 'info');

    try {
        setSessionWidgetState({
            profileId,
            profileName,
            phase: 'spin',
            status: 'Minecraft starting...',
            percent: null,
            canStop: true
        });

        // Listen for real progress events from the backend
        try {
            unlistenProgress = await window.__TAURI__.event.listen('launch-progress', (event) => {
                const { status, percent } = event.payload;
                const progressValue = Number(percent);
                const hasPercent = Number.isFinite(progressValue) && progressValue >= 0 && progressValue < 100;
                setSessionWidgetState({
                    profileId,
                    profileName,
                    phase: hasPercent ? 'progress' : 'spin',
                    status: status || 'Minecraft startet...',
                    percent: hasPercent ? progressValue : null,
                    canStop: true
                });
            });
        } catch (_e) {
            // Tauri events not available (e.g. in browser dev mode)
        }

        await invoke('launch_profile', {
            profileId: profileId,
            username: currentUsername
        });

        if (unlistenProgress) unlistenProgress();
        debugLog('Minecraft started successfully!', 'success');

        // Mark instance as running and update buttons immediately
        runningProfiles.add(profileId);
        updateAllPlayStopButtons(profileId);

        const stillRunning = await waitForMinecraftMenuReady(profileId, profileName);
        if (stillRunning) {
            setSessionWidgetState({
                profileId,
                profileName,
                phase: 'running',
                status: 'Minecraft started',
                percent: null,
                canStop: true
            });
        } else {
            setSessionWidgetState({ phase: 'idle', percent: null, canStop: false });
            syncSessionWidgetWithRunningProfiles();
        }
        if (liveLogProfileId === profileId) {
            const el = getLogContentElement();
            if (el) refreshLiveLog(profileId, el);
        }
    } catch (error) {
        if (typeof unlistenProgress === 'function') unlistenProgress();
        debugLog('Launch failed: ' + error, 'error');
        setSessionWidgetState({ phase: 'idle', percent: null, canStop: false });
        syncSessionWidgetWithRunningProfiles();
        showToast('Launch failed: ' + error, 'error', 5000);
    }
}

async function waitForMinecraftMenuReady(profileId, profileName) {
    const readyMarkers = [
        'Sound engine started',
        'OpenAL initialized',
        'Narrator library',
        'Reloading ResourceManager',
        'Created:'
    ];
    const timeoutMs = 150000;
    const pollMs = 2000;
    const start = Date.now();
    let tick = 0;

    while (Date.now() - start < timeoutMs) {
        // Instanz wurde gestoppt oder ist beim Start abgestürzt
        if (!runningProfiles.has(profileId)) return false;

        tick += 1;
        const elapsedRatio = Math.min((Date.now() - start) / timeoutMs, 1);
        const percent = 92 + Math.round(elapsedRatio * 7);

        setSessionWidgetState({
            profileId,
            profileName,
            phase: 'progress',
            status: 'Waiting for Minecraft to fully open...',
            percent,
            canStop: true
        });

        try {
            const logText = await invoke('get_profile_logs', { profileId, logType: 'latest' });
            const lower = String(logText || '').toLowerCase();
            const matched = readyMarkers.some(marker => lower.includes(marker.toLowerCase()));
            if (matched && tick >= 2) {
                return true;
            }
        } catch (_e) {
            // latest.log may briefly be missing right after startup.
        }

        await new Promise(r => setTimeout(r, pollMs));
    }
    return runningProfiles.has(profileId);
}

function showMicrosoftLoginInfo() {
    const modalHTML = `
        <div style="position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.8); display: flex; align-items: center; justify-content: center; z-index: 10000;" onclick="this.remove()">
            <div style="background: var(--bg-dark); border: 2px solid var(--gold); border-radius: 10px; padding: 30px; max-width: 600px; max-height: 80vh; overflow-y: auto;" onclick="event.stopPropagation()">
                <h2 style="color: var(--gold); margin: 0 0 20px 0;"><i class="bi bi-shield-lock"></i> Microsoft-Login (Coming Soon)</h2>
                
                <p style="color: var(--text-secondary); margin-bottom: 20px;">
                    The Lion Launcher will implement <strong style="color: var(--gold);">real Microsoft login</strong>!
                </p>
                
                <h3 style="color: var(--text-primary); margin-bottom: 10px;">How will it work?</h3>
                
                <ol style="color: var(--text-secondary); margin: 0 0 20px 20px; line-height: 1.8;">
                    <li><strong>OAuth2 flow</strong> - a browser opens with Microsoft login</li>
                    <li><strong>You sign in</strong> - with your real Microsoft account</li>
                    <li><strong>Token is stored</strong> - encrypted on your PC</li>
                    <li><strong>Automatic renewal</strong> - the token is refreshed automatically</li>
                    <li><strong>Offline support</strong> - cached token for offline play</li>
                </ol>
                
                <h3 style="color: var(--text-primary); margin-bottom: 10px;">What do you get?</h3>
                
                <ul style="color: var(--text-secondary); margin: 0 0 20px 20px; line-height: 1.8;">
                    <li><i class="bi bi-check-circle-fill" style="color:#4caf50;"></i> <strong>Your real Minecraft account</strong></li>
                    <li><i class="bi bi-check-circle-fill" style="color:#4caf50;"></i> <strong>Access to purchased skins & capes</strong></li>
                    <li><i class="bi bi-check-circle-fill" style="color:#4caf50;"></i> <strong>Multiplayer on all servers</strong></li>
                    <li><i class="bi bi-check-circle-fill" style="color:#4caf50;"></i> <strong>Realms support</strong></li>
                    <li><i class="bi bi-check-circle-fill" style="color:#4caf50;"></i> <strong>Account security</strong></li>
                </ul>
                
                <div style="background: var(--bg-light); border-left: 4px solid #4CAF50; padding: 15px; margin-top: 20px;">
                    <p style="color: var(--text-primary); margin: 0 0 5px 0;">
                        <strong><i class="bi bi-lock"></i> Security:</strong>
                    </p>
                    <p style="color: var(--text-secondary); margin: 0; font-size: 14px;">
                        Your password is <strong>NEVER</strong> stored in the launcher!<br>
                        Only Microsoft has access to your login data.<br>
                        The launcher only stores an encrypted access token.
                    </p>
                </div>
                
                <button class="btn" onclick="this.closest('div[style*=\\'position: fixed\\']').remove()" style="width: 100%; margin-top: 25px; padding: 12px;">
                    Schließen
                </button>
            </div>
        </div>
    `;

    const modalDiv = document.createElement('div');
    modalDiv.innerHTML = modalHTML;
    document.body.appendChild(modalDiv.firstElementChild);
}

async function deleteProfile(profileId) {
    closeDynamicModal('profile-context-menu');
    closeDynamicModal('share-profile-modal');

    const confirmed = await showDeleteProfileConfirmModal(profileId);
    if (!confirmed) {
        return;
    }

    try {
        const profileList = await invoke('delete_profile', { profileId });
        profiles = profileList.profiles || [];
        closeDynamicModal('profile-context-menu');
        closeDynamicModal('share-profile-modal');
        renderProfiles();
        showToast('Profile deleted.', 'success', 2200);
    } catch (error) {
        debugLog('Failed to delete profile: ' + error, 'error');
        showToast('Failed to delete profile: ' + error, 'error', 5000);
    }
}

function showDeleteProfileConfirmModal(profileId) {
    return new Promise((resolve) => {
        closeDynamicModal('delete-profile-confirm-modal');
        const profile = profiles.find(p => p.id === profileId);
        const profileName = profile ? profile.name : 'this profile';

        const modal = document.createElement('div');
        modal.id = 'delete-profile-confirm-modal';
        modal.style.cssText = 'position: fixed; inset: 0; background: rgba(0,0,0,0.82); display: flex; align-items: center; justify-content: center; z-index: 10002;';
        modal.innerHTML = `
            <div style="background: var(--bg-dark); border: 2px solid #f44336; border-radius: 12px; width: min(430px, 90vw); padding: 20px;" onclick="event.stopPropagation()">
                <h3 style="margin: 0 0 10px 0; color: #f44336; font-size: 20px;"><i class="bi bi-trash"></i> Delete profile?</h3>
                <p style="margin: 0 0 16px 0; color: var(--text-secondary); font-size: 13px;">Do you really want to delete <strong style="color: var(--text-primary);">${escapeHtml(profileName)}</strong>?</p>
                <div style="display: flex; justify-content: flex-end; gap: 10px;">
                    <button class="btn btn-secondary" id="delete-profile-cancel-btn">Cancel</button>
                    <button class="btn" id="delete-profile-confirm-btn" style="background:#f44336; color:#fff;">Yes I will</button>
                </div>
            </div>
        `;

        const cleanup = (result) => {
            if (modal.parentElement) modal.remove();
            resolve(result);
        };

        modal.addEventListener('click', () => cleanup(false));
        document.body.appendChild(modal);

        const cancelBtn = document.getElementById('delete-profile-cancel-btn');
        const confirmBtn = document.getElementById('delete-profile-confirm-btn');
        if (cancelBtn) cancelBtn.addEventListener('click', () => cleanup(false));
        if (confirmBtn) confirmBtn.addEventListener('click', () => cleanup(true));
    });
}

// Profile Details View
// `view` ({ mainCategory, subTab }) stellt beim Zurück/Vorwärts den zuletzt offenen Tab wieder her
function showProfileDetails(profileId, view = {}) {
    debugLog('Opening profile details: ' + profileId, 'info');

    const profile = profiles.find(p => p.id === profileId);
    if (!profile) {
        debugLog('Profile not found: ' + profileId, 'error');
        return;
    }

    recordNavigation({ type: 'profile', profileId });
    currentProfile = profile;
    currentProfileSubTab = 'mods'; // Reset auf Mods beim Öffnen eines Profils

    // Erstelle Details-View
    const grid = document.getElementById('profiles-grid');
    if (!grid) return;

    // Icon: if icon_path is present (data URL), show it, otherwise the app icon
    const iconHTML = profile.icon_path
        ? `<img src="${profile.icon_path}" alt="Profile Icon" style="width: 100%; height: 100%; object-fit: cover; border-radius: 12px;" data-fallback="logo">`
        : `<img src="${LION_LOGO_URL}" alt="Lion Launcher" style="width: 100%; height: 100%; object-fit: cover; border-radius: 12px;">`;

    const loaderLabel = SERVER_SOFTWARE[profile.loader.loader]?.label || profile.loader.loader;

    grid.innerHTML = `
        <div style="grid-column: 1 / -1;">
            <!-- Profil Header: das Icon reicht über beide Zeilen bis an den Content-Kasten -->
            <div style="display: grid; grid-template-columns: 230px minmax(0, 1fr); grid-template-rows: minmax(0, 1fr) auto;
                        column-gap: 22px; row-gap: 16px; margin-bottom: 20px;">
                <!-- Icon -->
                <div style="grid-column: 1; grid-row: 1 / span 2; width: 230px; height: 230px; display: flex; align-items: center; justify-content: center;
                            border-radius: 14px; overflow: hidden; background: var(--bg-light);">
                    ${iconHTML}
                </div>

                <!-- Rechts neben dem Icon: Play oben, Profil-Info unten -->
                <div style="grid-column: 2; grid-row: 1; min-width: 0; display: flex; flex-direction: column; justify-content: space-between; gap: 12px;">
                    <div style="display: flex; justify-content: flex-end;">
                        <button class="btn" data-play-btn="${profile.id}" onclick="launchProfile('${profile.id}')" style="padding: 15px 40px; font-size: 18px; flex-shrink: 0;">
                            <i class="bi bi-play-fill"></i> Play
                        </button>
                    </div>
                    <div style="min-width: 0; display: flex; flex-direction: column; gap: 8px;">
                        <h2 style="color: var(--gold); margin: 0; font-size: 24px; font-weight: 700; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;" title="${escapeAttr(profile.name)}">${escapeHtml(profile.name)}</h2>
                        <p style="margin: 0; color: var(--text-secondary); font-size: 14px;">
                            Minecraft ${escapeHtml(profile.minecraft_version)} • ${escapeHtml(loaderLabel)} ${escapeHtml(profile.loader.version || '')}
                        </p>
                    </div>
                </div>

                <!-- Hauptkategorien: mittig über dem Content-Kasten; ist das Fenster zu schmal,
                     schiebt die linke Spalte (Icon-Breite) sie gerade so weit nach rechts wie nötig -->
                <div style="grid-column: 1 / -1; grid-row: 2; display: grid; grid-template-columns: minmax(252px, 1fr) auto minmax(0, 1fr);">
                    <div style="grid-column: 2; background: var(--bg-medium); border-radius: 10px; padding: 8px; display: flex; gap: 8px;">
                        <button class="main-category-tab active" data-maincategory="content" onclick="switchMainCategory('content')" 
                                onmouseover="if(!this.classList.contains('active')) this.style.background='rgba(218, 165, 32, 0.1)'"
                                onmouseout="if(!this.classList.contains('active')) this.style.background='transparent'"
                                style="padding: 10px 24px; background: var(--gold); border: none; color: var(--bg-dark);
                                       cursor: pointer; border-radius: 8px; font-weight: 600; font-size: 15px;
                                       transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1); white-space: nowrap; transform: scale(1);">
                            Content
                        </button>
                        <button class="main-category-tab" data-maincategory="worlds" onclick="switchMainCategory('worlds')" 
                                onmouseover="if(!this.classList.contains('active')) this.style.background='rgba(218, 165, 32, 0.1)'"
                                onmouseout="if(!this.classList.contains('active')) this.style.background='transparent'"
                                style="padding: 10px 24px; background: transparent; border: none; color: var(--text-secondary);
                                       cursor: pointer; border-radius: 8px; font-weight: 600; font-size: 15px;
                                       transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1); white-space: nowrap; transform: scale(1);">
                            Worlds
                        </button>
                        <button class="main-category-tab" data-maincategory="servers" onclick="switchMainCategory('servers')" 
                                onmouseover="if(!this.classList.contains('active')) this.style.background='rgba(218, 165, 32, 0.1)'"
                                onmouseout="if(!this.classList.contains('active')) this.style.background='transparent'"
                                style="padding: 10px 24px; background: transparent; border: none; color: var(--text-secondary);
                                       cursor: pointer; border-radius: 8px; font-weight: 600; font-size: 15px;
                                       transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1); white-space: nowrap; transform: scale(1);">
                            Servers
                        </button>
                        <button class="main-category-tab" data-maincategory="logs" onclick="switchMainCategory('logs')" 
                                onmouseover="if(!this.classList.contains('active')) this.style.background='rgba(218, 165, 32, 0.1)'"
                                onmouseout="if(!this.classList.contains('active')) this.style.background='transparent'"
                                style="padding: 10px 24px; background: transparent; border: none; color: var(--text-secondary);
                                       cursor: pointer; border-radius: 8px; font-weight: 600; font-size: 15px;
                                       transition: all 0.3s cubic-bezier(0.4, 0, 0.2, 1); white-space: nowrap; transform: scale(1);">
                            Logs
                        </button>
                    </div>
                </div>
            </div>

            <!-- Content Area (unter dem Strich) -->
            <div id="main-category-shell" style="background: var(--bg-medium); border: 1px solid var(--bg-light); border-radius: 12px; padding: 12px; margin-bottom: 16px; box-shadow: 0 8px 20px rgba(0, 0, 0, 0.22);">
                <div id="main-category-content">
                    ${renderMainCategoryContent('content', profile)}
                </div>
            </div>
        </div>
    `;

    // Lade Content automatisch nach dem Rendern
    setTimeout(() => {
        if (view.mainCategory && view.mainCategory !== 'content') {
            switchMainCategory(view.mainCategory);
        } else if (view.subTab && view.subTab !== 'mods') {
            switchContentSubTab(view.subTab);
        } else {
            loadInstalledMods(profile.id);
            startModsWatcher(profile.id);
        }
        updateProfileScrollableBoxes();
        // Play/Stop-Button-State korrekt setzen
        updateAllPlayStopButtons(profile.id);
    }, 50);
}

function switchMainCategory(categoryName) {
    debugLog('Switching to main category: ' + categoryName, 'info');
    patchCurrentNavLocation(e => e.type === 'profile' && e.profileId === currentProfile?.id, { mainCategory: categoryName });

    // Update button styles mit Animation
    document.querySelectorAll('.main-category-tab').forEach(btn => {
        if (btn.dataset.maincategory === categoryName) {
            btn.style.background = 'var(--gold)';
            btn.style.color = 'var(--bg-dark)';
            btn.style.transform = 'scale(1.05)';
            btn.classList.add('active');
        } else {
            btn.style.background = 'transparent';
            btn.style.color = 'var(--text-secondary)';
            btn.style.transform = 'scale(1)';
            btn.classList.remove('active');
        }
    });

    // Fade-Out Animation für Content
    const content = document.getElementById('main-category-content');
    if (content && currentProfile) {
        // Fade out
        content.style.opacity = '0';
        content.style.transform = 'translateY(-10px)';
        content.style.transition = 'opacity 0.2s ease-out, transform 0.2s ease-out';

        // Nach Fade-Out: Content wechseln und Fade-In
        setTimeout(() => {
            content.innerHTML = renderMainCategoryContent(categoryName, currentProfile);

            // Fade in
            content.style.opacity = '1';
            content.style.transform = 'translateY(0)';
            content.style.transition = 'opacity 0.3s ease-in, transform 0.3s ease-in';

            // Lade kategorie-spezifische Daten
            if (categoryName === 'content') {
                loadInstalledMods(currentProfile.id);
                startModsWatcher(currentProfile.id);
                stopLogsAutoRefresh();
                stopLiveLog();
            } else if (categoryName === 'worlds') {
                loadWorlds(currentProfile.id);
                stopLogsAutoRefresh();
                stopLiveLog();
                stopModsWatcher();
            } else if (categoryName === 'servers') {
                loadServers(currentProfile.id);
                stopLogsAutoRefresh();
                stopLiveLog();
                stopModsWatcher();
            } else if (categoryName === 'logs') {
                stopModsWatcher();
                currentLogType = 'latest';
                setTimeout(() => {
                    loadLogs(currentProfile.id);
                    populateLogSourceDropdown(currentProfile.id);
                }, 80);
            } else {
                stopLogsAutoRefresh();
                stopLiveLog();
                stopModsWatcher();
            }
            updateProfileScrollableBoxes();
        }, 200);
    }
}

function renderMainCategoryContent(categoryName, profile) {
    switch (categoryName) {
        case 'content':
            return `
                <!-- Content Tab Navigation (Mods, ResourcePacks, ShaderPacks) - kleiner -->
                <div style="display: flex; gap: 8px; margin-bottom: 20px;">
                    <button class="content-sub-tab active" data-subtab="mods" onclick="switchContentSubTab('mods')" 
                            style="padding: 6px 14px; background: var(--bg-medium); border: 2px solid var(--gold); color: var(--text-primary); 
                                   cursor: pointer; border-radius: 6px; font-weight: 500; font-size: 12px; transition: all 0.2s;">
                        <i class="bi bi-puzzle"></i> Mods
                    </button>
                    <button class="content-sub-tab" data-subtab="resourcepacks" onclick="switchContentSubTab('resourcepacks')" 
                            style="padding: 6px 14px; background: var(--bg-medium); border: 2px solid var(--bg-light); color: var(--text-secondary); 
                                   cursor: pointer; border-radius: 6px; font-weight: 500; font-size: 12px; transition: all 0.2s;">
                        <i class="bi bi-palette"></i> Resource Packs
                    </button>
                    <button class="content-sub-tab" data-subtab="shaderpacks" onclick="switchContentSubTab('shaderpacks')" 
                            style="padding: 6px 14px; background: var(--bg-medium); border: 2px solid var(--bg-light); color: var(--text-secondary); 
                                   cursor: pointer; border-radius: 6px; font-weight: 500; font-size: 12px; transition: all 0.2s;">
                        <i class="bi bi-stars"></i> Shader Packs
                    </button>
                    <div style="flex: 1;"></div>
                    <button onclick="openContentBrowser('${profile.id}')" 
                            style="padding: 6px 16px; font-size: 12px; background: var(--bg-light); color: var(--gold); 
                                   border: 2px solid var(--gold); border-radius: 6px; cursor: pointer; font-weight: 600;
                                   transition: all 0.2s ease;"
                            onmouseover="this.style.background='var(--gold)'; this.style.color='var(--bg-dark)';"
                            onmouseout="this.style.background='var(--bg-light)'; this.style.color='var(--gold)';">
                        + Add Content
                    </button>
                </div>
                
                <!-- Sub Content Area -->
                <div id="content-sub-tab-content">
                    ${renderContentSubTab('mods', profile)}
                </div>
            `;

        case 'worlds':
            return `
                ${renderProfileTabContent('worlds', profile)}
            `;

        case 'servers':
            return `
                ${renderProfileTabContent('servers', profile)}
            `;

        case 'logs':
            return renderLogsContent(profile);

        default:
            return '<p style="text-align: center; color: var(--text-secondary); padding: 40px;">Inhalt nicht verfügbar</p>';
    }
}

function switchContentSubTab(subtabName) {
    debugLog('Switching to content sub-tab: ' + subtabName, 'info');

    if (currentProfileSubTab === subtabName && document.querySelector('.content-sub-tab.active')?.dataset.subtab === subtabName) {
        return;
    }

    // Speichere aktuellen Sub-Tab
    currentProfileSubTab = subtabName;
    patchCurrentNavLocation(e => e.type === 'profile' && e.profileId === currentProfile?.id, { subTab: subtabName });

    // Update button styles
    document.querySelectorAll('.content-sub-tab').forEach(btn => {
        if (btn.dataset.subtab === subtabName) {
            btn.style.borderColor = 'var(--gold)';
            btn.style.color = 'var(--text-primary)';
            btn.classList.add('active');
        } else {
            btn.style.borderColor = 'var(--bg-light)';
            btn.style.color = 'var(--text-secondary)';
            btn.classList.remove('active');
        }
    });

    // Update content
    const content = document.getElementById('content-sub-tab-content');
    if (content && currentProfile) {
        content.innerHTML = renderContentSubTab(subtabName, currentProfile);

        // Lade Tab-spezifische Daten
        if (subtabName === 'mods') {
            loadInstalledMods(currentProfile.id);
            startModsWatcher(currentProfile.id);
        } else if (subtabName === 'resourcepacks') {
            loadInstalledResourcePacks(currentProfile.id);
        } else if (subtabName === 'shaderpacks') {
            loadInstalledShaderPacks(currentProfile.id);
        } else if (subtabName === 'worlds') {
            loadWorlds(currentProfile.id);
        } else if (subtabName === 'servers') {
            loadServers(currentProfile.id);
        }
        updateProfileScrollableBoxes();
    }
}

function renderContentSubTab(subtabName, profile) {
    // Nutze die gleiche Rendering-Logik wie vorher
    return renderProfileTabContent(subtabName, profile);
}

function renderLogsContent(profile) {
    return `
        <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 15px;">
            <h3 style="color: var(--gold); margin: 0;">Minecraft Logs</h3>
            <div style="display: flex; gap: 10px;">
                <button class="btn btn-secondary" onclick="copyLogsToClipboard()" style="padding: 8px 16px;">
                    <i class="bi bi-clipboard"></i> Kopieren
                </button>
                <button class="btn btn-secondary" onclick="refreshCurrentLog('${profile.id}')" style="padding: 8px 16px;">
                    <i class="bi bi-arrow-clockwise"></i> Aktualisieren
                </button>
                <button class="btn btn-secondary" onclick="openLogsFolder('${profile.id}')" style="padding: 8px 16px;">
                    <i class="bi bi-folder"></i> Ordner
                </button>
            </div>
        </div>

        <!-- Tab-Leiste: Logs | Debug | Crash Reports -->
        <div style="display: flex; gap: 8px; margin-bottom: 12px;">
            <button class="log-type-btn active" data-log="logs"
                    onclick="switchLogTab('logs', '${profile.id}')"
                    style="padding: 8px 16px; background: var(--gold); color: var(--bg-dark);
                           border: none; border-radius: 6px; font-weight: 500; font-size: 13px; cursor: pointer;">
                <i class="bi bi-file-text"></i> Logs
            </button>
            <button class="log-type-btn" data-log="debug"
                    onclick="switchLogTab('debug', '${profile.id}')"
                    style="padding: 8px 16px; background: var(--bg-medium); border: 2px solid var(--bg-light);
                           color: var(--text-secondary); border-radius: 6px; font-size: 13px; cursor: pointer;">
                <i class="bi bi-search"></i> Debug
            </button>
            <button class="log-type-btn" data-log="crash"
                    onclick="switchLogTab('crash', '${profile.id}')"
                    style="padding: 8px 16px; background: var(--bg-medium); border: 2px solid var(--bg-light);
                           color: var(--text-secondary); border-radius: 6px; font-size: 13px; cursor: pointer;">
                <i class="bi bi-lightning-charge"></i> Crash Reports
            </button>
        </div>

        <!-- Stats-Bar mit Dropdown -->
        <div id="log-stats-bar" style="display: flex; align-items: center; gap: 15px; margin-bottom: 10px;
                font-size: 11px; color: var(--text-secondary); padding: 8px 12px;
                background: var(--bg-medium); border-radius: 6px;">
            <span id="log-line-count">Zeilen: -</span>
            <span id="log-error-count" style="color: #f44336;">Errors: -</span>
            <span id="log-warn-count" style="color: #ff9800;">Warnings: -</span>
            <div style="flex: 1;"></div>
            <select id="log-source-select"
                    style="background: var(--bg-dark); border: 1px solid var(--bg-light); border-radius: 5px;
                           color: var(--text-secondary); padding: 4px 8px; font-size: 11px; cursor: pointer; max-width: 230px;"
                    onchange="switchLogSource('${profile.id}', this.value)">
                <option value="live">🔴 Live Log</option>
                <option value="latest" selected>📄 Latest</option>
            </select>
        </div>

        <div id="logs-container" class="profile-scroll-box" style="background: #0d0d0d; border-radius: 8px; padding: 10px;
                height: 420px; max-height: 420px; margin-bottom: 12px;
                overflow-y: auto; overflow-x: hidden; font-family: 'JetBrains Mono', 'Fira Code', 'Courier New', monospace;
                font-size: 11px; line-height: 1.4; color: #e0e0e0; user-select: text; border: 1px solid var(--bg-light);">
            <div style="text-align: center; padding: 40px; color: var(--text-secondary);">
                <div class="spinner" style="margin: 0 auto 15px;"></div>
                <p>Lade Logs...</p>
            </div>
        </div>
    `;
}

function switchProfileTab(tabName) {
    debugLog('Switching to tab: ' + tabName, 'info');

    // Update tab styles
    document.querySelectorAll('.profile-tab').forEach(tab => {
        if (tab.dataset.tab === tabName) {
            tab.style.color = 'var(--text-primary)';
            tab.style.borderBottom = '3px solid var(--gold)';
            tab.classList.add('active');
        } else {
            tab.style.color = 'var(--text-secondary)';
            tab.style.borderBottom = '3px solid transparent';
            tab.classList.remove('active');
        }
    });

    // Update content
    const content = document.getElementById('profile-tab-content');
    if (content && currentProfile) {
        content.innerHTML = renderProfileTabContent(tabName, currentProfile);

        // Lade Tab-spezifische Daten
        if (tabName === 'mods') {
            loadInstalledMods(currentProfile.id);
            startModsWatcher(currentProfile.id);
        } else if (tabName === 'logs') {
            loadLogs(currentProfile.id);
        } else if (tabName === 'resourcepacks') {
            loadInstalledResourcePacks(currentProfile.id);
        } else if (tabName === 'shaderpacks') {
            loadInstalledShaderPacks(currentProfile.id);
        }
        updateProfileScrollableBoxes();
    }
}

function renderProfileTabContent(tabName, profile) {
    switch (tabName) {
        case 'mods':
            return `
                <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px;">
                    <h3 style="color: var(--gold); margin: 0;">Installierte Mods</h3>
                    <div style="display: flex; gap: 8px;">
                        <button id="mod-updates-btn" class="btn btn-secondary" onclick="checkForModUpdates('${profile.id}')" style="padding: 8px 12px; font-size: 12px;">
                            <i class="bi bi-search"></i> Updates
                        </button>
                        <button class="btn btn-secondary" onclick="refreshInstalledMods('${profile.id}')" style="padding: 8px 12px; font-size: 12px;">
                            <i class="bi bi-arrow-clockwise"></i>
                        </button>
                        <button class="btn btn-secondary" onclick="openModsFolder('${profile.id}')" style="padding: 8px 12px; font-size: 12px;">
                            <i class="bi bi-folder"></i>
                        </button>
                    </div>
                </div>

                <!-- Suchleiste für installierte Mods -->
                <div style="position: relative; margin-bottom: 12px;">
                    <i class="bi bi-search" style="position: absolute; left: 10px; top: 50%; transform: translateY(-50%); color: var(--text-secondary); font-size: 13px; pointer-events: none;"></i>
                    <input type="text" id="installed-mods-search"
                           placeholder="${t('search_mods_placeholder')}"
                           oninput="filterInstalledMods(this.value)"
                           style="width: 100%; padding: 8px 10px 8px 32px; background: var(--bg-light);
                                  border: 1px solid var(--bg-medium); border-radius: 6px;
                                  color: var(--text-primary); font-size: 13px; box-sizing: border-box;
                                  outline: none; transition: border-color 0.2s;"
                           onfocus="this.style.borderColor='var(--gold)'"
                           onblur="this.style.borderColor='var(--bg-medium)'">
                </div>
                
                <!-- Bulk Actions Bar (hidden by default) -->
                <div id="bulk-actions-bar" style="display: none; background: var(--bg-light); border-radius: 8px; padding: 10px 15px; margin-bottom: 12px; align-items: center; gap: 15px;">
                    <label style="display: flex; align-items: center; gap: 8px; cursor: pointer; color: var(--text-secondary);">
                        <input type="checkbox" id="select-all-mods" onchange="toggleSelectAllMods()" style="width: 16px; height: 16px; cursor: pointer;">
                        <span id="selected-count">0 ausgewählt</span>
                    </label>
                    <div style="flex: 1;"></div>
                    <button class="btn btn-secondary" onclick="bulkActivateMods('${profile.id}')" style="padding: 6px 12px; font-size: 11px;">
                        <i class="bi bi-check-circle"></i> Aktivieren
                    </button>
                    <button class="btn btn-secondary" onclick="bulkDeactivateMods('${profile.id}')" style="padding: 6px 12px; font-size: 11px;">
                        <i class="bi bi-pause-circle"></i> Deaktivieren
                    </button>
                    <button class="btn btn-secondary" onclick="bulkDeleteMods('${profile.id}')" style="padding: 6px 12px; font-size: 11px; color: #f44336;">
                        <i class="bi bi-trash"></i> Delete
                    </button>
                </div>
                
                <div id="profile-mods-list" class="profile-scroll-box" style="display: grid; gap: 8px; height: 420px; max-height: 420px; overflow-y: auto; overflow-x: hidden; padding-right: 5px;">
                    <div style="text-align: center; padding: 40px; color: var(--text-secondary);">
                        <div class="spinner" style="margin: 0 auto 15px;"></div>
                        <p>${t('loading_mods')}</p>
                    </div>
                </div>
            `;

        case 'resourcepacks':
            // Stoppe Mods-Watcher wenn Tab gewechselt wird
            stopModsWatcher();
            return `
                <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 15px;">
                    <h3 style="color: var(--gold); margin: 0;">Resource Packs</h3>
                    <div style="display: flex; gap: 8px;">
                        <button class="btn btn-secondary" onclick="refreshResourcePacks('${profile.id}')" style="padding: 8px 12px; font-size: 12px;">
                            <i class="bi bi-arrow-clockwise"></i>
                        </button>
                        <button class="btn btn-secondary" onclick="openResourcePacksFolder('${profile.id}')" style="padding: 8px 12px; font-size: 12px;">
                            <i class="bi bi-folder"></i>
                        </button>
                    </div>
                </div>
                
                <div id="profile-resourcepacks-list" class="profile-scroll-box" style="display: grid; gap: 8px; height: 420px; max-height: 420px; overflow-y: auto; overflow-x: hidden; padding-right: 5px;">
                    <div style="text-align: center; padding: 40px; color: var(--text-secondary);">
                        <div class="spinner" style="margin: 0 auto 15px;"></div>
                        <p>Lade Resource Packs...</p>
                    </div>
                </div>
            `;

        case 'shaderpacks':
            stopModsWatcher();
            return `
                <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 15px;">
                    <h3 style="color: var(--gold); margin: 0;">Shader Packs</h3>
                    <div style="display: flex; gap: 8px;">
                        <button class="btn btn-secondary" onclick="refreshShaderPacks('${profile.id}')" style="padding: 8px 12px; font-size: 12px;">
                            <i class="bi bi-arrow-clockwise"></i>
                        </button>
                        <button class="btn btn-secondary" onclick="openShaderPacksFolder('${profile.id}')" style="padding: 8px 12px; font-size: 12px;">
                            <i class="bi bi-folder"></i>
                        </button>
                    </div>
                </div>
                
                <div id="profile-shaderpacks-list" class="profile-scroll-box" style="display: grid; gap: 8px; height: 420px; max-height: 420px; overflow-y: auto; overflow-x: hidden; padding-right: 5px;">
                    <div style="text-align: center; padding: 40px; color: var(--text-secondary);">
                        <div class="spinner" style="margin: 0 auto 15px;"></div>
                        <p>Lade Shader Packs...</p>
                    </div>
                </div>
            `;

        case 'worlds':
            stopModsWatcher();
            return `
                <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 15px;">
                    <h3 style="color: var(--gold); margin: 0;"><i class="bi bi-globe"></i> Welten</h3>
                    <div style="display: flex; gap: 8px;">
                        <button class="btn btn-secondary" onclick="refreshWorlds('${profile.id}')" style="padding: 8px 12px; font-size: 12px;">
                            <i class="bi bi-arrow-clockwise"></i>
                        </button>
                        <button class="btn btn-secondary" onclick="openWorldsFolder('${profile.id}')" style="padding: 8px 12px; font-size: 12px;">
                            <i class="bi bi-folder"></i>
                        </button>
                    </div>
                </div>
                
                <div id="profile-worlds-list" class="profile-scroll-box" style="display: grid; gap: 8px; align-content: start; grid-auto-rows: min-content; height: 420px; max-height: 420px; overflow-y: auto; overflow-x: hidden; padding-right: 5px;">
                    <div style="text-align: center; padding: 40px; color: var(--text-secondary);">
                        <div class="spinner" style="margin: 0 auto 15px;"></div>
                        <p>Lade Welten...</p>
                    </div>
                </div>
            `;

        case 'servers':
            stopModsWatcher();
            return `
                <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 15px;">
                    <h3 style="color: var(--gold); margin: 0;"><i class="bi bi-display"></i> Server</h3>
                    <div style="display: flex; gap: 8px;">
                        <button class="btn btn-secondary" onclick="showAddServerModal()"
                                style="padding: 8px 12px; font-size: 12px; border: 2px solid var(--gold); color: var(--gold); background: var(--bg-light);"
                                onmouseover="this.style.background='var(--gold)'; this.style.color='var(--bg-dark)';"
                                onmouseout="this.style.background='var(--bg-light)'; this.style.color='var(--gold)';">
                            <i class="bi bi-plus-lg"></i> Server hinzufügen
                        </button>
                        <button class="btn btn-secondary" onclick="refreshServers('${profile.id}')" style="padding: 8px 12px; font-size: 12px;">
                            <i class="bi bi-arrow-clockwise"></i>
                        </button>
                    </div>
                </div>
                
                <div id="profile-servers-list" class="profile-scroll-box" style="display: grid; gap: 8px; height: 420px; max-height: 420px; overflow-y: auto; overflow-x: hidden; padding-right: 5px;">
                    <div style="text-align: center; padding: 40px; color: var(--text-secondary);">
                        <div class="spinner" style="margin: 0 auto 15px;"></div>
                        <p>Lade Server...</p>
                    </div>
                </div>
                
                <!-- Add Server Modal -->
                <div id="add-server-modal" class="modal">
                    <div class="modal-content" style="max-width: 450px;">
                        <div class="modal-header"><i class="bi bi-plus-circle"></i> Server hinzufügen</div>
                        <div class="form-group">
                            <label>Servername</label>
                            <input type="text" id="add-server-name" placeholder="z.B. Mein Server" style="background: var(--bg-dark);">
                        </div>
                        <div class="form-group">
                            <label>Server-Adresse (IP / Domain)</label>
                            <input type="text" id="add-server-ip" placeholder="z.B. mc.hypixel.net oder 192.168.1.1:25565" style="background: var(--bg-dark);">
                        </div>
                        <div class="modal-actions">
                            <button class="btn btn-secondary" onclick="hideAddServerModal()">Cancel</button>
                            <button class="btn" onclick="submitAddServer('${profile.id}')"><i class="bi bi-plus-lg"></i> Hinzufügen</button>
                        </div>
                    </div>
                </div>
            `;

        case 'logs':
            stopModsWatcher();
            stopLiveLog();
            return `
                <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 16px;">
                    <h3 style="color: var(--gold); margin: 0;">Minecraft Logs</h3>
                    <div style="display: flex; gap: 10px;">
                        <button class="btn btn-secondary" onclick="refreshCurrentLog('${profile.id}')" style="padding: 8px 16px;">
                            <i class="bi bi-arrow-clockwise"></i> Aktualisieren
                        </button>
                        <button class="btn btn-secondary" onclick="copyLogsToClipboard()" style="padding: 8px 16px;">
                            <i class="bi bi-clipboard"></i> Kopieren
                        </button>
                        <button class="btn btn-secondary" onclick="openLogsFolder('${profile.id}')" style="padding: 8px 16px;">
                            <i class="bi bi-folder"></i> Ordner
                        </button>
                    </div>
                </div>

                <!-- Tab-Leiste: Logs | Debug | Crash Reports -->
                <div style="display: flex; gap: 8px; margin-bottom: 10px;">
                    <button class="log-type-btn active" data-log="logs"
                            onclick="switchLogTab('logs', '${profile.id}')"
                            style="padding: 8px 18px; background: var(--gold); color: var(--bg-dark);
                                   border: none; border-radius: 5px; cursor: pointer; font-weight: 600; font-size: 13px;">
                        <i class="bi bi-file-text"></i> Logs
                    </button>
                    <button class="log-type-btn" data-log="debug"
                            onclick="switchLogTab('debug', '${profile.id}')"
                            style="padding: 8px 18px; background: var(--bg-dark); color: var(--text-secondary);
                                   border: 1px solid var(--bg-light); border-radius: 5px; cursor: pointer; font-size: 13px;">
                        <i class="bi bi-search"></i> Debug
                    </button>
                    <button class="log-type-btn" data-log="crash"
                            onclick="switchLogTab('crash', '${profile.id}')"
                            style="padding: 8px 18px; background: var(--bg-dark); color: var(--text-secondary);
                                   border: 1px solid var(--bg-light); border-radius: 5px; cursor: pointer; font-size: 13px;">
                        <i class="bi bi-lightning-charge"></i> Crash Reports
                    </button>
                </div>

                <!-- Stats-Bar mit Log-Quellen-Dropdown ganz rechts -->
                <div id="log-stats-bar"
                     style="display: flex; align-items: center; gap: 15px; margin-bottom: 8px;
                            padding: 7px 12px; background: var(--bg-medium); border-radius: 6px;
                            font-size: 11px; color: var(--text-secondary);">
                    <span id="log-line-count">Zeilen: -</span>
                    <span id="log-error-count" style="color: #f44336;">Errors: -</span>
                    <span id="log-warn-count" style="color: #ff9800;">Warnings: -</span>
                    <div style="flex: 1;"></div>
                    <!-- Dropdown: Live Log oben, dann Latest, dann ältere .gz Dateien -->
                    <select id="log-source-select"
                            style="background: var(--bg-dark); border: 1px solid var(--bg-light);
                                   border-radius: 5px; color: var(--text-secondary); padding: 4px 8px;
                                   font-size: 11px; cursor: pointer; max-width: 230px;"
                            onchange="switchLogSource('${profile.id}', this.value)">
                        <option value="live">🔴 Live Log</option>
                        <option value="latest" selected>📄 Latest</option>
                    </select>
                </div>

                <!-- Log Content -->
                <div id="log-content" class="profile-scroll-box"
                     style="background: #0d0d0d; border: 1px solid var(--bg-light); border-radius: 6px;
                            padding: 12px; font-family: 'Courier New', monospace; font-size: 11px;
                            height: 420px; max-height: 420px; margin-bottom: 12px;
                            overflow-y: auto; overflow-x: hidden; color: #ccc; word-break: break-all;">
                    <div style="color: var(--gold); text-align: center; padding: 40px;">⏳ Lade Logs…</div>
                </div>
            `;

        default:
            return '<p style="text-align: center; color: var(--text-secondary); padding: 40px;">Inhalt nicht verfügbar</p>';
    }
}

// Helper functions for profile details

/**
 * Platz, den die Elemente UNTER der Box bis zum Ende von `stopEl` brauchen
 * (Geschwister darunter, Paddings/Borders/Margins der Eltern).
 * Bewusst unabhängig von der aktuellen Höhe der Box selbst – sonst kann eine einmal
 * verkleinerte Box (z.B. nach Maximieren/Wiederherstellen) nie wieder wachsen.
 */
function measureSpaceBelowBox(box, stopEl) {
    const px = value => parseFloat(value) || 0;
    let space = 0;
    let el = box;
    while (el && el !== stopEl && el.parentElement) {
        space += px(getComputedStyle(el).marginBottom);
        const parent = el.parentElement;
        const pcs = getComputedStyle(parent);
        const stacksVertically = pcs.display === 'block' || pcs.display === 'flow-root' ||
            ((pcs.display === 'flex' || pcs.display === 'inline-flex') && pcs.flexDirection.startsWith('column'));
        if (stacksVertically) {
            for (let sib = el.nextElementSibling; sib; sib = sib.nextElementSibling) {
                const scs = getComputedStyle(sib);
                if (scs.display === 'none' || scs.position === 'absolute' || scs.position === 'fixed') continue;
                space += sib.getBoundingClientRect().height + px(scs.marginTop) + px(scs.marginBottom);
            }
        }
        space += px(pcs.paddingBottom) + px(pcs.borderBottomWidth);
        el = parent;
    }
    return space;
}

function updateProfileScrollableBoxes() {
    const boxes = document.querySelectorAll('.profile-scroll-box');
    if (!boxes.length) return;

    boxes.forEach(box => {
        if (box.offsetParent === null) return;
        const rect = box.getBoundingClientRect();

        // Server-Detailansicht hat einen eigenen Scroll-Container mit fester Maximalhöhe
        const scrollHost = box.closest('#server-detail-content');
        const stopEl = scrollHost || box.closest('.content') || document.body;
        let bottomLimit = window.innerHeight;
        let boxTop = rect.top;
        if (scrollHost) {
            const hostMax = parseFloat(getComputedStyle(scrollHost).maxHeight);
            if (Number.isFinite(hostMax)) {
                bottomLimit = Math.min(bottomLimit, scrollHost.getBoundingClientRect().top + hostMax);
            }
            boxTop += scrollHost.scrollTop;
        }

        const available = Math.floor(bottomLimit - boxTop - measureSpaceBelowBox(box, stopEl) - 4);
        const targetHeight = Math.max(180, Math.min(available, 520));
        box.style.height = `${targetHeight}px`;
        box.style.maxHeight = `${targetHeight}px`;
    });
}

/**
 * Beim Maximieren/Wiederherstellen feuert der WebView teils mehrere resize-Events mit
 * Zwischengrößen. Daher sofort, im nächsten Frame und nach kurzer Verzögerung nachmessen.
 */
let scrollBoxResizeTimer = null;
function scheduleProfileScrollableBoxesUpdate() {
    updateProfileScrollableBoxes();
    requestAnimationFrame(updateProfileScrollableBoxes);
    clearTimeout(scrollBoxResizeTimer);
    scrollBoxResizeTimer = setTimeout(updateProfileScrollableBoxes, 180);
}

// Öffnet den Haupt-Ordner des Profils
async function openProfileFolder(profileId) {
    debugLog('Opening profile folder for: ' + profileId, 'info');
    try {
        await invoke('open_profile_folder', { profileId: profileId, subfolder: null });
        showToast('Ordner wird geöffnet...', 'info', 2000);
    } catch (error) {
        debugLog('Failed to open profile folder: ' + error, 'error');
        showToast('Fehler beim Öffnen: ' + error, 'error', 3000);
    }
}

// Öffnet den Game-Ordner (gleich wie Profil-Ordner)
async function openGameFolder(profileId) {
    debugLog('Opening game folder for: ' + profileId, 'info');
    try {
        await invoke('open_profile_folder', { profileId: profileId, subfolder: null });
        showToast('Ordner wird geöffnet...', 'info', 2000);
    } catch (error) {
        debugLog('Failed to open game folder: ' + error, 'error');
        showToast('Fehler beim Öffnen: ' + error, 'error', 3000);
    }
}

// Öffnet den Logs-Ordner
async function openLogsFolder(profileId) {
    debugLog('Opening logs folder for: ' + profileId, 'info');
    try {
        await invoke('open_profile_folder', { profileId: profileId, subfolder: 'logs' });
        showToast('Logs-Ordner wird geöffnet...', 'info', 2000);
    } catch (error) {
        debugLog('Failed to open logs folder: ' + error, 'error');
        showToast('Fehler beim Öffnen: ' + error, 'error', 3000);
    }
}

let currentLogType = 'latest';

async function loadLogs(profileId) {
    debugLog('Loading logs for profile: ' + profileId, 'info');
    await new Promise(resolve => setTimeout(resolve, 50));

    if (currentLogType === 'latest' && isLiveLogSelected()) return;

    let targetElement = document.getElementById('logs-container') || document.getElementById('log-content');
    if (!targetElement) {
        await new Promise(resolve => setTimeout(resolve, 100));
        targetElement = document.getElementById('logs-container') || document.getElementById('log-content');
        if (!targetElement) return;
    }

    targetElement.innerHTML = '<div style="color:var(--gold);text-align:center;padding:20px;">⏳ Lade Logs…</div>';

    try {
        const logs = await invoke('get_profile_logs', {
            profileId: profileId,
            logType: currentLogType
        });

        if (currentLogType === 'latest' && isLiveLogSelected()) return;

        if (!logs || logs.trim().length === 0) {
            targetElement.innerHTML = `
                <div style="color:var(--text-secondary);text-align:center;padding:40px;">
                    <i class="bi bi-clipboard"></i> Keine ${currentLogType} Logs gefunden<br>
                    <span style="font-size:11px;color:#666;">Starte Minecraft um Logs zu generieren</span>
                </div>`;
            return;
        }

        renderLogContent(targetElement, logs);
        targetElement.scrollTop = targetElement.scrollHeight;
        updateLogStats(logs);
        debugLog('Logs loaded: ' + logs.split('\n').length + ' lines', 'success');

        if (currentLogType === 'latest') {
            populateLogSourceDropdown(profileId);
        }
    } catch (error) {
        targetElement.innerHTML = `
            <div style="padding:20px;">
                <div style="color:#f44336;margin-bottom:10px;"><i class="bi bi-x-circle-fill"></i> Fehler beim Laden</div>
                <pre style="color:var(--text-secondary);font-size:11px;white-space:pre-wrap;">${escapeHtml(String(error))}</pre>
            </div>`;
    }
}

function getLogContentElement() {
    return document.getElementById('logs-container') || document.getElementById('log-content');
}

function isLiveLogSelected() {
    const sel = document.getElementById('log-source-select');
    return !!sel && sel.value === 'live';
}

// ── Log-Tab und Log-Quellen-Steuerung ────────────────────────────────────────

/**
 * Schaltet zwischen den drei Haupt-Tabs: logs / debug / crash
 */
function switchLogTab(tabName, profileId) {
    stopLiveLog();

    // Tab-Button-Styles
    document.querySelectorAll('.log-type-btn').forEach(btn => {
        const active = btn.dataset.log === tabName;
        btn.style.background = active ? 'var(--gold)' : 'var(--bg-dark)';
        btn.style.color = active ? 'var(--bg-dark)' : 'var(--text-secondary)';
        btn.style.border = active ? 'none' : '1px solid var(--bg-light)';
        btn.classList.toggle('active', active);
    });

    // Stats-Bar + Dropdown nur beim Logs-Tab sichtbar
    const statsBar = document.getElementById('log-stats-bar');
    if (statsBar) statsBar.style.display = tabName === 'logs' ? 'flex' : 'none';

    const pid = profileId || (currentProfile ? currentProfile.id : null);

    if (tabName === 'logs') {
        // Dropdown zurücksetzen auf "latest" und Dropdown befüllen
        const sel = document.getElementById('log-source-select');
        if (sel) sel.value = 'latest';
        currentLogType = 'latest';
        loadLogs(pid);
        populateLogSourceDropdown(pid);
    } else {
        currentLogType = tabName; // 'debug' oder 'crash'
        loadLogs(pid);
    }
}

/**
 * Dropdown-Auswahl: live / latest / file:name.log.gz
 */
async function switchLogSource(profileId, value) {
    stopLiveLog();
    const pid = profileId || (currentProfile ? currentProfile.id : null);

    if (value === 'live') {
        // Live-Dot im Dropdown-Eintrag animieren (visuelles Feedback)
        const sel = document.getElementById('log-source-select');
        if (sel) sel.style.borderColor = '#f44336';
        currentLogType = 'latest';
        await startLiveLog(pid);
    } else if (value === 'latest') {
        const sel = document.getElementById('log-source-select');
        if (sel) sel.style.borderColor = '';
        currentLogType = 'latest';
        loadLogs(pid);
    } else if (value && value.startsWith('file:')) {
        const sel = document.getElementById('log-source-select');
        if (sel) sel.style.borderColor = '';
        // Ältere .gz/.log Datei laden
        const filename = value.slice(5);
        const el = getLogContentElement();
        if (!el) return;
        el.innerHTML = `<div style="color:var(--gold);text-align:center;padding:20px;">⏳ Lade ${escapeHtml(filename)}…</div>`;
        invoke('get_profile_logs', { profileId: pid, logType: value })
            .then(content => { renderLogContent(el, content); updateLogStats(content); })
            .catch(e => { el.innerHTML = `<div style="color:#f44336;padding:20px;">Fehler: ${escapeHtml(String(e))}</div>`; });
    }
}

/**
 * Befüllt das Dropdown mit: 🔴 Live Log, 📄 Latest, dann ältere Dateien (neueste zuerst)
 */
async function populateLogSourceDropdown(profileId) {
    const sel = document.getElementById('log-source-select');
    if (!sel || !profileId) return;

    // Fixe Einträge oben behalten
    const currentVal = sel.value || 'latest';
    sel.innerHTML = `
        <option value="live">🔴 Live Log</option>
        <option value="latest">📄 Latest</option>
    `;

    try {
        const files = await invoke('get_log_files', { profileId });
        files.forEach(name => {
            const opt = document.createElement('option');
            opt.value = 'file:' + name;
            opt.textContent = '🗜 ' + name;
            sel.appendChild(opt);
        });
    } catch (e) { /* silent */ }

    // Wert wiederherstellen falls noch gültig
    if ([...sel.options].some(o => o.value === currentVal)) sel.value = currentVal;
    else sel.value = 'latest';
}

/** Aktualisiert den aktuell sichtbaren Log (respektiert Live/Latest/Older) */
async function refreshCurrentLog(profileId) {
    const sel = document.getElementById('log-source-select');
    const val = sel ? sel.value : 'latest';
    if (val === 'live') {
        // Live ist aktiv – einfach manuell einmal nachladen
        const el = getLogContentElement();
        if (el) {
            await syncRunningProfiles();
            refreshLiveLog(profileId || (currentProfile ? currentProfile.id : null), el);
        }
    } else {
        switchLogSource(profileId, val || 'latest');
    }
}

/** Stoppt den Live-Log-Refresh */
function stopLiveLog() {
    if (liveLogInterval) {
        clearInterval(liveLogInterval);
        liveLogInterval = null;
    }
    liveLogProfileId = null;
    liveLogLastSnapshot = '';
    // Dropdown-Rand zurücksetzen
    const sel = document.getElementById('log-source-select');
    if (sel) sel.style.borderColor = '';
}

/** Startet den Live-Log-Refresh (schnell, mit Delta-Rendering) */
async function startLiveLog(profileId) {
    if (!profileId) return;
    const el = getLogContentElement();
    if (!el) return;

    liveLogProfileId = profileId;
    liveLogLastSnapshot = '';

    // Dropdown-Rand rot markieren
    const sel = document.getElementById('log-source-select');
    if (sel) sel.style.borderColor = '#f44336';

    el.innerHTML = `<div style="color:var(--gold);text-align:center;padding:20px;">⏳ Live Log wird gestartet…</div>`;

    await syncRunningProfiles();

    // Sofort laden
    await refreshLiveLog(profileId, el);

    liveLogInterval = setInterval(() => {
        const container = getLogContentElement();
        if (!container) { stopLiveLog(); return; }
        refreshLiveLog(profileId, container);
    }, 500);
}

async function refreshLiveLog(profileId, el) {
    if (!el || !profileId) return;

    const isRunning = runningProfiles.has(profileId);

    if (!isRunning) {
        liveLogLastSnapshot = '';
        el.innerHTML = `
            <div style="color:var(--text-secondary);text-align:center;padding:40px;">
                <i class="bi bi-stop-circle" style="font-size:32px;color:#f44336;display:block;margin-bottom:10px;"></i>
                <span style="color:#f44336;font-weight:600;">🔴 Keine Instanz läuft</span><br>
                <span style="font-size:11px;margin-top:6px;display:block;">Live Log ist nur verfügbar, wenn Minecraft für dieses Profil gestartet wurde.</span>
                <span style="font-size:11px;margin-top:4px;display:block;color:var(--gold);">Tipp: Über „📄 Latest“ kannst du den letzten gespeicherten Log ansehen.</span>
            </div>`;
        updateLogStats('');
        return;
    }

    try {
        const content = await invoke('get_live_launcher_logs', { limit: 800 });

        const noContent = !content || !content.trim();

        if (noContent) {
            liveLogLastSnapshot = '';
            el.innerHTML = `
                <div style="color:var(--text-secondary);text-align:center;padding:40px;">
                    <i class="bi bi-broadcast" style="font-size:32px;color:#4caf50;display:block;
                        margin-bottom:10px;animation:livePulse 1s infinite;"></i>
                    <span style="color:#4caf50;font-weight:600;">🟢 Instanz läuft</span><br>
                    <span style="font-size:11px;margin-top:6px;display:block;">Warte auf Log-Ausgabe…</span>
                </div>`;
            updateLogStats('');
            return;
        }

        // Log-Inhalt vorhanden → Status-Banner + Log rendern
        const statusBanner = `<div style="display:flex;align-items:center;gap:8px;padding:6px 10px;margin-bottom:6px;
                background:rgba(76,175,80,0.12);border:1px solid rgba(76,175,80,0.3);border-radius:5px;
                font-size:11px;color:#4caf50;font-weight:600;">
                <span style="width:8px;height:8px;border-radius:50%;background:#4caf50;display:inline-block;
                    animation:livePulse 1s infinite;"></span>
                LIVE – Instanz läuft
           </div>`;

        if (content === liveLogLastSnapshot) {
            return;
        }
        liveLogLastSnapshot = content;

        const wasAtBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 80;

        // Log rendern
        renderLogContent(el, content);
        updateLogStats(content);

        // Status-Banner vorne einfügen
        el.insertAdjacentHTML('afterbegin', statusBanner);

        // Nur nach unten scrollen wenn läuft UND schon am Ende war
        if (isRunning && wasAtBottom) el.scrollTop = el.scrollHeight;

    } catch (e) { /* silent */ }
}
// ─────────────────────────────────────────────────────────────────────────────


function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

// Kopiert die aktuellen Logs in die Zwischenablage
async function copyLogsToClipboard() {
    // Verwende die gespeicherten Roh-Logs für bessere Lesbarkeit
    if (!currentRawLogs || currentRawLogs.trim().length === 0) {
        showToast('Keine Logs zum Kopieren gefunden', 'error', 2000);
        return;
    }

    try {
        await navigator.clipboard.writeText(currentRawLogs);
        showToast('Logs in Zwischenablage kopiert!', 'success', 2000);
        debugLog('Logs copied to clipboard: ' + currentRawLogs.length + ' characters', 'success');
    } catch (error) {
        debugLog('Failed to copy logs: ' + error, 'error');
        showToast('Fehler beim Kopieren: ' + error, 'error', 3000);
    }
}

// Variable zum Speichern der Roh-Logs für Statistiken
let currentRawLogs = '';

/**
 * Rendert einen Log-String als formatierte HTML-Zeilen in ein Container-Element.
 * Wird von loadLogs() und refreshLiveLog() verwendet.
 */
function renderLogContent(targetElement, logs) {
    if (!logs || logs.trim().length === 0) return;

    // Hilfsnachrichten direkt ausgeben
    if (logs.startsWith('📋') || logs.startsWith('📄') || logs.startsWith('⚠️')) {
        targetElement.innerHTML = `<pre style="color:var(--gold);white-space:pre-wrap;font-family:monospace;font-size:12px;line-height:1.6;">${escapeHtml(logs)}</pre>`;
        return;
    }

    const formattedLogs = logs.split('\n').map((line, index) => {
        if (!line.trim()) return '';
        const timeMatch = line.match(/^\[(\d{2}:\d{2}:\d{2})]/);
        let timeStamp = '';
        let restOfLine = line;
        if (timeMatch) {
            timeStamp = timeMatch[1];
            restOfLine = line.substring(timeMatch[0].length);
        }
        let levelColor = '#b0b0b0';
        let levelBadge = '';
        if (line.includes('[ERROR]') || line.includes('/ERROR]') || line.includes('Exception') || line.includes('FATAL')) {
            levelColor = '#f44336';
            levelBadge = '<span style="background:#f44336;color:white;padding:1px 5px;border-radius:3px;font-size:9px;margin-right:6px;">ERROR</span>';
        } else if (line.includes('[WARN]') || line.includes('/WARN]') || line.includes('Warning')) {
            levelColor = '#ff9800';
            levelBadge = '<span style="background:#ff9800;color:black;padding:1px 5px;border-radius:3px;font-size:9px;margin-right:6px;">WARN</span>';
        } else if (line.includes('[INFO]') || line.includes('/INFO]')) {
            levelColor = '#4caf50';
            levelBadge = '<span style="background:#4caf50;color:white;padding:1px 5px;border-radius:3px;font-size:9px;margin-right:6px;">INFO</span>';
        } else if (line.includes('[DEBUG]') || line.includes('/DEBUG]')) {
            levelColor = '#9e9e9e';
            levelBadge = '<span style="background:#616161;color:white;padding:1px 5px;border-radius:3px;font-size:9px;margin-right:6px;">DEBUG</span>';
        }
        restOfLine = restOfLine
            .replace(/\[(?:INFO|WARN|ERROR|DEBUG)]/g, '')
            .replace(/\[main\/(?:INFO|WARN|ERROR)]/g, '')
            .replace(/\[Render thread\/(?:INFO|WARN|ERROR)]/g, '')
            .trim();
        const timeDisplay = timeStamp
            ? `<span style="color:#888;font-weight:500;min-width:70px;display:inline-block;">${timeStamp}</span>`
            : '';
        return `<div style="display:flex;align-items:flex-start;padding:2px 4px;border-radius:3px;" data-line="${index}"
                    onmouseover="this.style.background='rgba(255,255,255,0.05)'"
                    onmouseout="this.style.background='transparent'">
            ${timeDisplay}${levelBadge}
            <span style="color:${levelColor};flex:1;word-break:break-word;">${escapeHtml(restOfLine)}</span>
        </div>`;
    }).filter(l => l).join('');

    targetElement.innerHTML = `<div style="display:flex;flex-direction:column;">${formattedLogs}</div>`;
    currentRawLogs = logs;
}

// Aktualisiert die Log-Statistiken Anzeige
function updateLogStats(logs) {
    const lines = logs.split('\n').filter(l => l.trim());
    const lineCount = lines.length;
    const errorCount = lines.filter(l => l.includes('[ERROR]') || l.includes('/ERROR]') || l.includes('Exception') || l.includes('FATAL')).length;
    const warnCount = lines.filter(l => l.includes('[WARN]') || l.includes('/WARN]')).length;

    const lineCountEl = document.getElementById('log-line-count');
    const errorCountEl = document.getElementById('log-error-count');
    const warnCountEl = document.getElementById('log-warn-count');

    if (lineCountEl) lineCountEl.textContent = `Zeilen: ${lineCount.toLocaleString()}`;
    if (errorCountEl) {
        errorCountEl.textContent = `Errors: ${errorCount}`;
        errorCountEl.style.fontWeight = errorCount > 0 ? 'bold' : 'normal';
    }
    if (warnCountEl) {
        warnCountEl.textContent = `Warnings: ${warnCount}`;
        warnCountEl.style.fontWeight = warnCount > 0 ? 'bold' : 'normal';
    }
}

function refreshLogs() {
    if (currentProfile) {
        debugLog('Manually refreshing logs', 'info');
        loadLogs(currentProfile.id);
    }
}

// Auto-Refresh für Logs
let logsRefreshInterval = null;

function startLogsAutoRefresh(profileId) {
    stopLogsAutoRefresh();

    debugLog('Starting logs auto-refresh', 'info');

    // Aktualisiere alle 3 Sekunden
    logsRefreshInterval = setInterval(() => {
        if (currentProfile && currentProfile.id === profileId) {
            // Nur aktualisieren wenn auf der Logs-Seite
            const logsTab = document.querySelector('[data-category="logs"].active');
            if (logsTab) {
                loadLogs(profileId);
            }
        }
    }, 3000);
}

function stopLogsAutoRefresh() {
    if (logsRefreshInterval) {
        clearInterval(logsRefreshInterval);
        logsRefreshInterval = null;
        debugLog('Stopped logs auto-refresh', 'info');
    }
}

async function clearLogs() {
    const confirmed = await showConfirmModal({
        title: 'Logs löschen',
        message: 'Möchtest du wirklich alle Logs löschen',
        confirmText: 'Löschen'
    });
    if (confirmed) {
        debugLog('Clearing logs...', 'info');
        // TODO: Implement log clearing
    }
}

// ==================== MOD-VERWALTUNG ====================

let selectedMods = new Set();
let modsWatcherInterval = null;
let lastModsHash = '';
let modUpdateCandidates = [];
let selectedModUpdates = new Set();
let currentModUpdateProfileId = null;

// Mod-Updates werden per Datei-Hash bei Modrinth geprüft (check_mod_updates).
// Die zuerst sichtbaren Mods kommen in den ersten Batch, der Rest läuft parallel hinterher.
const MOD_UPDATE_FIRST_BATCH = 12;
const MOD_UPDATE_BATCH_SIZE = 40;
const MOD_UPDATE_PARALLEL_BATCHES = 3;
const MOD_UPDATE_FRESH_MS = 60 * 1000;
/** profileId -> { minecraftVersion, loader, checkedAt, results: Map<filename, status> } */
const modUpdateStatusCache = new Map();
/** profileId -> laufende Hintergrund-Prüfung */
const modUpdateChecksInFlight = new Map();
const modUpdateRecheckQueued = new Set();

// Startet Auto-Refresh für Mods-Ordner
function startModsWatcher(profileId) {
    // Stoppe vorherigen Watcher falls vorhanden
    stopModsWatcher();

    debugLog('Starting mods folder watcher for profile: ' + profileId, 'info');

    // Prüfe alle 3 Sekunden auf Änderungen
    modsWatcherInterval = setInterval(async () => {
        try {
            const mods = await invoke('get_installed_mods', { profileId });
            const newHash = generateModsHash(mods);

            if (lastModsHash && newHash !== lastModsHash) {
                debugLog('Mods folder changed, reloading...', 'info');
                invalidateProfileContentCaches(profileId);
                loadInstalledMods(profileId, { force: true });
            }
            lastModsHash = newHash;
        } catch (e) {
            // Ignoriere Fehler beim Polling
        }
    }, 3000);
}

function stopModsWatcher() {
    if (modsWatcherInterval) {
        clearInterval(modsWatcherInterval);
        modsWatcherInterval = null;
        lastModsHash = '';
        debugLog('Mods folder watcher stopped', 'info');
    }
}

function generateModsHash(mods) {
    if (!mods || mods.length === 0) return 'empty';
    return mods.map(m => `${m.filename}:${m.disabled}`).sort().join('|');
}

async function loadInstalledMods(profileId, options = {}) {
    debugLog('Loading installed mods for profile: ' + profileId, 'info');

    const modsList = document.getElementById('profile-mods-list');
    if (!modsList) return;

    const forceReload = !!options.force;
    selectedMods.clear();
    updateBulkActionsBar();

    const cached = profileModsCache.get(profileId);
    if (!forceReload && Array.isArray(cached)) {
        installedModsCache = cached;
        cachedModsProfileId = profileId;

        const searchInput = document.getElementById('installed-mods-search');
        if (searchInput) searchInput.value = '';

        if (cached.length === 0) {
            modsList.innerHTML = `
                <div style="text-align: center; padding: 60px 20px; color: var(--text-secondary);">
                    <div style="font-size: 48px; margin-bottom: 15px;"><i class="bi bi-box"></i></div>
                    <p>${t('no_mods_installed')}</p>
                    <p style="font-size: 14px; margin-top: 10px;">
                        Gehe zum <a href="#" onclick="navigateToPage('mods'); return false;" style="color: var(--gold);">Mod Browser</a> um Mods zu installieren
                    </p>
                </div>
            `;
            document.getElementById('bulk-actions-bar').style.display = 'none';
            updateProfileScrollableBoxes();
            return;
        }

        document.getElementById('bulk-actions-bar').style.display = 'flex';
        renderInstalledModsList(cached, profileId);
        updateProfileScrollableBoxes();
        refreshModUpdateStatuses(profileId, cached);
        return;
    }

    try {
        const mods = await invoke('get_installed_mods', { profileId });

        // Cache für Suchfilter speichern
        installedModsCache = mods || [];
        cachedModsProfileId = profileId;
        profileModsCache.set(profileId, installedModsCache);
        installedModIdsProfileId = null;
        installedModIds.clear();

        // Suchfeld zurücksetzen beim Neuladen
        const searchInput = document.getElementById('installed-mods-search');
        if (searchInput) searchInput.value = '';

        if (!mods || mods.length === 0) {
            modsList.innerHTML = `
                <div style="text-align: center; padding: 60px 20px; color: var(--text-secondary);">
                    <div style="font-size: 48px; margin-bottom: 15px;"><i class="bi bi-box"></i></div>
                    <p>${t('no_mods_installed')}</p>
                    <p style="font-size: 14px; margin-top: 10px;">
                        Gehe zum <a href="#" onclick="navigateToPage('mods'); return false;" style="color: var(--gold);">Mod Browser</a> um Mods zu installieren
                    </p>
                </div>
            `;
            document.getElementById('bulk-actions-bar').style.display = 'none';
            updateProfileScrollableBoxes();
            return;
        }

        debugLog('Found ' + mods.length + ' installed mods', 'success');

        // Zeige Bulk Actions Bar
        document.getElementById('bulk-actions-bar').style.display = 'flex';

        renderInstalledModsList(mods, profileId);

        updateProfileScrollableBoxes();

        // Updates, echte Namen und Icons kommen asynchron per Hash-Abgleich mit Modrinth
        refreshModUpdateStatuses(profileId, mods, { force: !!options.recheckUpdates });

    } catch (error) {
        debugLog('Failed to load installed mods: ' + error, 'error');
        modsList.innerHTML = `
            <div style="text-align: center; padding: 40px; color: #f44336;">
                <div style="font-size: 48px; margin-bottom: 15px; color: #f44336;"><i class="bi bi-x-circle-fill"></i></div>
                <p>Fehler beim Laden der Mods</p>
                <p style="font-size: 12px; color: var(--text-secondary);">${error}</p>
                <button class="btn btn-secondary" onclick="loadInstalledMods('${profileId}', { force: true })" style="margin-top: 15px;">
                    <i class="bi bi-arrow-clockwise"></i> Try again
                </button>
            </div>
        `;
        updateProfileScrollableBoxes();
    }
}

/** Filtert die installierte Mod-Liste anhand des Suchbegriffs (client-seitig) */
function filterInstalledMods(query) {
    if (!cachedModsProfileId) return;

    const q = query.trim().toLowerCase();
    const filtered = q.length === 0
        ? installedModsCache
        : installedModsCache.filter(mod => {
            const name = (mod.name || mod.filename || '').toLowerCase();
            const filename = (mod.filename || '').toLowerCase();
            const version = (mod.version || '').toLowerCase();
            return name.includes(q) || filename.includes(q) || version.includes(q);
        });

    const modsList = document.getElementById('profile-mods-list');
    if (!modsList) return;

    if (filtered.length === 0) {
        modsList.innerHTML = `
            <div style="text-align: center; padding: 40px; color: var(--text-secondary);">
                <i class="bi bi-search" style="font-size: 32px; display: block; margin-bottom: 12px;"></i>
                <p>${t('no_mods_found')}</p>
            </div>
        `;
        return;
    }

    renderInstalledModsList(filtered, cachedModsProfileId);
}

/** Rendert eine Liste von installierten Mods in #profile-mods-list */
function renderInstalledModsList(mods, profileId) {
    const modsList = document.getElementById('profile-mods-list');
    if (!modsList) return;

    modsList.innerHTML = mods.map(mod => {
        const iconUrl = mod.icon_url || null;
        const file = escapeAttr(mod.filename);
        const jsFile = escapeAttr(String(mod.filename).replace(/\\/g, '\\\\').replace(/'/g, "\\'"));
        const updateStatus = getModUpdateStatus(profileId, mod.filename);

        return `
        <div class="installed-mod-card${mod.disabled ? ' is-disabled' : ''}" data-filename="${file}" data-mod-id="${escapeAttr(updateStatus?.project_id || mod.mod_id || '')}"
             onclick="if(!event.target.closest('input, button')) { showModDetailsFromProfile(this.dataset.modId, 'modrinth'); }">
            <!-- Checkbox -->
            <input type="checkbox" class="mod-checkbox" data-filename="${file}"
                   onchange="toggleModSelection('${jsFile}')"
                   onclick="event.stopPropagation();"
                   style="width: 18px; height: 18px; cursor: pointer; flex-shrink: 0; accent-color: var(--gold);">

            <!-- Icon -->
            <div class="mod-card-icon" style="width: 44px; height: 44px; background: var(--bg-light); border-radius: 6px; display: flex; align-items: center; justify-content: center; flex-shrink: 0; overflow: hidden;">
                ${iconUrl
                ? `<img alt="" src="${escapeAttr(iconUrl)}" style="width: 100%; height: 100%; object-fit: cover;" data-fallback="icon">`
                : `<span style="font-size: 22px;"><i class="bi bi-box"></i></span>`
            }
            </div>

            <!-- Info -->
            <div style="flex: 1; min-width: 0;">
                <div style="display: flex; align-items: center; gap: 8px; flex-wrap: wrap;">
                    <h4 class="mod-card-name" style="margin: 0; color: var(--text-primary); font-size: 14px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 200px;">
                        ${escapeHtml(mod.name || mod.filename)}
                    </h4>
                    ${mod.disabled ? '<span style="background: #f44336; color: white; font-size: 9px; padding: 2px 5px; border-radius: 3px;">DEAKTIVIERT</span>' : ''}
                </div>
                <p style="margin: 3px 0 0 0; color: var(--text-secondary); font-size: 11px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">
                    ${escapeHtml(mod.filename)}
                </p>
                <p class="mod-card-version" style="margin: 2px 0 0 0; color: var(--gold); font-size: 10px;">${renderModVersionLine(mod, updateStatus)}</p>
            </div>

            <!-- Actions -->
            <div style="display: flex; align-items: center; gap: 6px; flex-shrink: 0;">
                <span class="mod-update-slot" style="display: contents;">${renderModUpdateButton(profileId, updateStatus)}</span>
                <button class="btn btn-secondary content-action-btn" onclick="event.stopPropagation(); toggleMod('${escapeAttr(profileId)}', '${jsFile}', ${mod.disabled})"
                        title="${mod.disabled ? 'Aktivieren' : 'Deaktivieren'}">
                    ${mod.disabled ? '<i class="bi bi-check-lg"></i>' : '||'}
                </button>
                <button class="btn btn-secondary content-action-btn content-action-delete" onclick="event.stopPropagation(); deleteMod('${escapeAttr(profileId)}', '${jsFile}')"
                        title="Löschen">
                    ×
                </button>
            </div>
        </div>
    `}).join('');
}

function renderModVersionLine(mod, updateStatus) {
    const current = updateStatus?.current_version || mod.version;
    if (updateStatus?.status === 'update' && updateStatus.latest_version) {
        return `v${escapeHtml(current || '?')} <strong>→ ${escapeHtml(updateStatus.latest_version)}</strong>`;
    }
    return current ? `v${escapeHtml(current)}` : '';
}

function renderModUpdateButton(profileId, updateStatus) {
    if (updateStatus?.status !== 'update' || !updateStatus.latest_version_id) return '';
    if (modsBeingUpdated.has(modUpdateJobKey(profileId, updateStatus.filename))) {
        return `
        <button class="btn btn-secondary mod-update-btn" disabled title="Wird aktualisiert...">
            <i class="bi bi-arrow-repeat lion-spin"></i>
        </button>`;
    }
    const title = updateStatus.current_compatible
        ? `Update auf ${updateStatus.latest_version} installieren`
        : `Auf ${updateStatus.latest_version} wechseln (passend für die Minecraft-Version des Profils)`;
    return `
        <button class="btn btn-secondary mod-update-btn" onclick="event.stopPropagation(); updateSingleMod(this)" title="${escapeAttr(title)}">
            <i class="bi bi-download"></i>
        </button>`;
}

function refreshInstalledMods(profileId) {
    invalidateProfileContentCaches(profileId);
    loadInstalledMods(profileId, { force: true, recheckUpdates: true });
}

// Mod-Auswahl für Bulk-Operationen
function toggleModSelection(filename) {
    if (selectedMods.has(filename)) {
        selectedMods.delete(filename);
    } else {
        selectedMods.add(filename);
    }
    updateBulkActionsBar();
}

function toggleSelectAllMods() {
    const checkboxes = document.querySelectorAll('.mod-checkbox');
    const selectAllCheckbox = document.getElementById('select-all-mods');

    if (selectAllCheckbox.checked) {
        checkboxes.forEach(cb => {
            cb.checked = true;
            selectedMods.add(cb.dataset.filename);
        });
    } else {
        checkboxes.forEach(cb => {
            cb.checked = false;
        });
        selectedMods.clear();
    }
    updateBulkActionsBar();
}

function updateBulkActionsBar() {
    const countEl = document.getElementById('selected-count');
    if (countEl) {
        countEl.textContent = `${selectedMods.size} ausgewählt`;
    }
}

async function bulkActivateMods(profileId) {
    if (selectedMods.size === 0) {
        showToast('Bitte wähle zuerst Mods aus', 'warning', 3000);
        return;
    }

    const count = selectedMods.size;
    debugLog('Bulk activating ' + count + ' mods', 'info');

    try {
        await invoke('bulk_toggle_mods', {
            profileId,
            filenames: Array.from(selectedMods),
            enable: true
        });
        debugLog('Mods activated', 'success');
        selectedMods.clear();
        invalidateProfileContentCaches(profileId);
        loadInstalledMods(profileId, { force: true });
        showToast(`${count} Mods aktiviert!`, 'success', 3000);
    } catch (error) {
        debugLog('Failed to activate mods: ' + error, 'error');
        showToast('Fehler beim Aktivieren: ' + error, 'error', 3000);
    }
}

async function bulkDeactivateMods(profileId) {
    if (selectedMods.size === 0) {
        showToast('Bitte wähle zuerst Mods aus', 'warning', 3000);
        return;
    }

    const count = selectedMods.size;
    debugLog('Bulk deactivating ' + count + ' mods', 'info');

    try {
        await invoke('bulk_toggle_mods', {
            profileId,
            filenames: Array.from(selectedMods),
            enable: false
        });
        debugLog('Mods deactivated', 'success');
        selectedMods.clear();
        invalidateProfileContentCaches(profileId);
        loadInstalledMods(profileId, { force: true });
        showToast(`${count} Mods deaktiviert!`, 'success', 3000);
    } catch (error) {
        debugLog('Failed to deactivate mods: ' + error, 'error');
        showToast('Fehler beim Deaktivieren: ' + error, 'error', 3000);
    }
}

async function bulkDeleteMods(profileId) {
    if (selectedMods.size === 0) {
        showToast('Bitte wähle zuerst Mods aus', 'warning', 3000);
        return;
    }


    const count = selectedMods.size;
    debugLog('Bulk deleting ' + count + ' mods', 'info');

    try {
        await invoke('bulk_delete_mods', {
            profileId,
            filenames: Array.from(selectedMods)
        });
        debugLog('Mods deleted', 'success');
        await loadInstalledModIds(true); // Cache aktualisieren
        selectedMods.clear();
        invalidateProfileContentCaches(profileId);
        loadInstalledMods(profileId, { force: true });
        showToast(`${count} Mods gelöscht!`, 'success', 3000);
    } catch (error) {
        debugLog('Failed to delete mods: ' + error, 'error');
        showToast('Fehler beim Löschen: ' + error, 'error', 3000);
    }
}

// ==================== MOD-UPDATES ====================

/** Cache-Eintrag eines Profils, verworfen sobald MC-Version oder Loader nicht mehr passen. */
function getModUpdateEntry(profileId) {
    const entry = modUpdateStatusCache.get(profileId);
    const profile = profiles.find(p => p.id === profileId);
    if (entry && profile && (entry.minecraftVersion !== profile.minecraft_version || entry.loader !== profile.loader?.loader)) {
        modUpdateStatusCache.delete(profileId);
        return null;
    }
    return entry || null;
}

function getModUpdateStatus(profileId, filename) {
    return getModUpdateEntry(profileId)?.results.get(filename) || null;
}

function storeModUpdateResults(profileId, check) {
    let entry = modUpdateStatusCache.get(profileId);
    if (!entry || entry.minecraftVersion !== check.minecraft_version || entry.loader !== check.loader) {
        entry = { minecraftVersion: check.minecraft_version, loader: check.loader, checkedAt: 0, results: new Map() };
        modUpdateStatusCache.set(profileId, entry);
    }

    const mods = profileModsCache.get(profileId) || [];
    (check.results || []).forEach(result => {
        entry.results.set(result.filename, result);
        // Echte Namen/Icons/Projekt-IDs auch für Suche und Detailansicht übernehmen
        const mod = mods.find(m => m.filename === result.filename);
        if (mod && result.project_id) {
            mod.mod_id = result.project_id;
            if (result.mod_name) mod.name = result.mod_name;
            if (!mod.icon_url && result.icon_url) mod.icon_url = result.icon_url;
        }
    });
    return entry;
}

function splitModUpdateBatches(filenames) {
    const batches = [];
    if (filenames.length > 0) batches.push(filenames.slice(0, MOD_UPDATE_FIRST_BATCH));
    for (let i = MOD_UPDATE_FIRST_BATCH; i < filenames.length; i += MOD_UPDATE_BATCH_SIZE) {
        batches.push(filenames.slice(i, i + MOD_UPDATE_BATCH_SIZE));
    }
    return batches;
}

/** Prüft Dateien batchweise: den ersten (sichtbaren) Batch sofort, den Rest parallel danach. */
async function runModUpdateCheck(profileId, filenames, onBatch) {
    const [firstBatch, ...remaining] = splitModUpdateBatches(filenames);
    if (!firstBatch) return;
    onBatch(await invoke('check_mod_updates', { profileId, filenames: firstBatch }));

    let nextIndex = 0;
    let firstError = null;
    const worker = async () => {
        while (nextIndex < remaining.length) {
            const batch = remaining[nextIndex++];
            try {
                onBatch(await invoke('check_mod_updates', { profileId, filenames: batch }));
            } catch (error) {
                firstError = firstError || error;
            }
        }
    };
    const workerCount = Math.min(MOD_UPDATE_PARALLEL_BATCHES, remaining.length);
    await Promise.all(Array.from({ length: workerCount }, worker));
    if (firstError) throw firstError;
}

/**
 * Hält die Update-Symbole der Mod-Liste aktuell: Ergebnisse der letzten Minute werden
 * sofort angezeigt (neue Dateien direkt nachgeprüft), sonst wird alles neu geprüft.
 */
function refreshModUpdateStatuses(profileId, mods, { force = false } = {}) {
    const filenames = (mods || []).map(mod => mod.filename);
    const entry = getModUpdateEntry(profileId);
    if (entry) {
        const present = new Set(filenames);
        for (const filename of entry.results.keys()) {
            if (!present.has(filename)) entry.results.delete(filename);
        }
    }
    applyModUpdateStatusesToList(profileId);

    if (modUpdateChecksInFlight.has(profileId)) {
        modUpdateRecheckQueued.add(profileId);
        return;
    }

    const isFresh = !!entry && Date.now() - entry.checkedAt < MOD_UPDATE_FRESH_MS;
    const toCheck = isFresh && !force ? filenames.filter(name => !entry.results.has(name)) : filenames;
    if (toCheck.length === 0) {
        updateModUpdatesButton(profileId);
        return;
    }

    const isFullCheck = toCheck.length === filenames.length;
    const startedAt = Date.now();
    const check = runModUpdateCheck(profileId, toCheck, result => {
        storeModUpdateResults(profileId, result);
        applyModUpdateStatusesToList(profileId, result.results);
    })
        .then(() => {
            const current = modUpdateStatusCache.get(profileId);
            if (current && isFullCheck) current.checkedAt = startedAt;
        })
        .catch(error => debugLog('Mod update check failed: ' + error, 'error'))
        .finally(() => {
            modUpdateChecksInFlight.delete(profileId);
            updateModUpdatesButton(profileId);
            if (modUpdateRecheckQueued.delete(profileId) && cachedModsProfileId === profileId) {
                refreshModUpdateStatuses(profileId, installedModsCache);
            }
        });
    modUpdateChecksInFlight.set(profileId, check);
    updateModUpdatesButton(profileId);
}

function applyModUpdateStatusesToList(profileId, results = null) {
    const list = document.getElementById('profile-mods-list');
    if (!list || cachedModsProfileId !== profileId) return;

    const statuses = results || Array.from(getModUpdateEntry(profileId)?.results.values() || []);
    statuses.forEach(status => {
        const card = list.querySelector(`.installed-mod-card[data-filename="${CSS.escape(status.filename)}"]`);
        if (card) applyModUpdateStatusToCard(card, status, profileId);
    });
    updateModUpdatesButton(profileId);
}

function applyModUpdateStatusToCard(card, status, profileId) {
    const slot = card.querySelector('.mod-update-slot');
    if (slot) slot.innerHTML = renderModUpdateButton(profileId, status);

    const versionLine = card.querySelector('.mod-card-version');
    if (versionLine && status.current_version) versionLine.innerHTML = renderModVersionLine({}, status);

    if (status.project_id) card.dataset.modId = status.project_id;

    const nameEl = card.querySelector('.mod-card-name');
    if (nameEl && status.mod_name && nameEl.textContent.trim() !== status.mod_name) {
        nameEl.textContent = status.mod_name;
    }

    const iconEl = card.querySelector('.mod-card-icon');
    if (iconEl && status.icon_url && !iconEl.querySelector('img') && iconEl.dataset.iconTried !== status.icon_url) {
        iconEl.dataset.iconTried = status.icon_url;
        iconEl.innerHTML = `<img alt="" src="${escapeAttr(status.icon_url)}" style="width: 100%; height: 100%; object-fit: cover;" data-fallback="icon">`;
    }
}

function updateModUpdatesButton(profileId) {
    const button = document.getElementById('mod-updates-btn');
    if (!button || currentProfile?.id !== profileId) return;

    const checking = modUpdateChecksInFlight.has(profileId);
    const entry = getModUpdateEntry(profileId);
    const count = entry
        ? Array.from(entry.results.values()).filter(s => s.status === 'update' && s.latest_version_id).length
        : 0;
    const icon = checking ? 'bi-arrow-repeat lion-spin' : count > 0 ? 'bi-download' : 'bi-search';

    button.innerHTML = `<i class="bi ${icon}"></i> Updates${count > 0 ? ` <span class="mod-update-count">${count}</span>` : ''}`;
    button.title = checking
        ? 'Suche nach Updates...'
        : count > 0 ? `${count} Update${count === 1 ? '' : 's'} verfügbar` : 'Nach Updates suchen';
}

/** Installiert das Update einer Mod und trägt die neue Datei als aktuell in den Cache ein. */
async function installModUpdate(profileId, status) {
    const newFilename = await invoke('update_mod', {
        profileId,
        filename: status.filename,
        versionId: status.latest_version_id
    });

    const entry = modUpdateStatusCache.get(profileId);
    if (entry) {
        entry.results.delete(status.filename);
        entry.results.set(newFilename, {
            ...status,
            filename: newFilename,
            status: 'up_to_date',
            current_version: status.latest_version,
            current_game_versions: status.latest_game_versions || [],
            current_compatible: true,
            latest_version: null,
            latest_version_id: null,
            latest_game_versions: []
        });
    }
    return newFilename;
}

// ── Mod-Updates installieren: eine Warteschlange, Fortschritt im Aktivitäts-Kasten ──
const MOD_UPDATE_PARALLEL_DOWNLOADS = 3;
const modUpdateQueue = {
    pending: [],
    active: 0,
    done: 0,
    total: 0,
    succeeded: [],
    failed: [],
    lastError: null,
    currentName: null,
    profileIds: new Set(),
    lastProfileId: null
};
/** "profileId/filename" aller Mods, deren Update wartet oder gerade läuft */
const modsBeingUpdated = new Set();

function modUpdateJobKey(profileId, filename) {
    return `${profileId}/${filename}`;
}

function getModDisplayName(profileId, status) {
    if (status.mod_name || status.display_name) return status.mod_name || status.display_name;
    const mod = (profileModsCache.get(profileId) || []).find(m => m.filename === status.filename);
    return mod?.name || status.filename;
}

/** Stellt Updates in die Warteschlange; Mods, die schon aktualisiert werden, werden übersprungen. */
function enqueueModUpdates(profileId, statuses) {
    const jobs = statuses.filter(status => status?.latest_version_id &&
        !modsBeingUpdated.has(modUpdateJobKey(profileId, status.filename)));
    if (jobs.length === 0) return;

    jobs.forEach(status => {
        modsBeingUpdated.add(modUpdateJobKey(profileId, status.filename));
        modUpdateQueue.pending.push({ profileId, status, name: getModDisplayName(profileId, status) });
    });
    modUpdateQueue.total += jobs.length;
    modUpdateQueue.profileIds.add(profileId);
    modUpdateQueue.lastProfileId = profileId;

    // Spinner statt Download-Symbol, bis das jeweilige Update fertig ist
    applyModUpdateStatusesToList(profileId, jobs);
    pumpModUpdateQueue();
    renderModUpdateActivity();
}

function pumpModUpdateQueue() {
    while (modUpdateQueue.active < MOD_UPDATE_PARALLEL_DOWNLOADS && modUpdateQueue.pending.length > 0) {
        const job = modUpdateQueue.pending.shift();
        modUpdateQueue.active += 1;
        modUpdateQueue.currentName = job.name;
        runModUpdateJob(job);
    }
}

async function runModUpdateJob(job) {
    let succeeded = false;
    try {
        await installModUpdate(job.profileId, job.status);
        modUpdateQueue.succeeded.push(job);
        succeeded = true;
    } catch (error) {
        debugLog(`Mod update failed (${job.status.filename}): ${error}`, 'error');
        modUpdateQueue.failed.push(job.name);
        modUpdateQueue.lastError = String(error);
    } finally {
        modsBeingUpdated.delete(modUpdateJobKey(job.profileId, job.status.filename));
        refreshModCardAfterUpdate(job, succeeded);
        modUpdateQueue.active -= 1;
        modUpdateQueue.done += 1;
        if (modUpdateQueue.active === 0 && modUpdateQueue.pending.length === 0) {
            finishModUpdateQueue();
        } else {
            pumpModUpdateQueue();
            renderModUpdateActivity();
        }
    }
}

/** Zeigt das Ergebnis sofort an der Karte, ohne auf das Neuladen der Liste zu warten. */
function refreshModCardAfterUpdate(job, succeeded) {
    const list = document.getElementById('profile-mods-list');
    if (!list || cachedModsProfileId !== job.profileId) return;
    const card = list.querySelector(`.installed-mod-card[data-filename="${CSS.escape(job.status.filename)}"]`);
    if (!card) return;

    if (!succeeded) {
        // Download-Symbol wieder anbieten
        applyModUpdateStatusToCard(card, job.status, job.profileId);
        return;
    }
    const slot = card.querySelector('.mod-update-slot');
    if (slot) slot.innerHTML = '';
    const versionLine = card.querySelector('.mod-card-version');
    if (versionLine) versionLine.innerHTML = `v${escapeHtml(job.status.latest_version || '')} <i class="bi bi-check2"></i>`;
}

function renderModUpdateActivity() {
    const { done, total, lastProfileId } = modUpdateQueue;
    if (total === 0) return;
    const single = total === 1;
    setSessionWidgetState({
        profileId: lastProfileId,
        profileName: getProfileNameById(lastProfileId),
        phase: single ? 'spin' : 'progress',
        status: single ? `Aktualisiere ${modUpdateQueue.currentName}...` : `Aktualisiere Mods... (${done}/${total})`,
        percent: single ? null : (done / total) * 100,
        canStop: false,
        activity: 'mod-update'
    });
}

function finishModUpdateQueue() {
    const succeeded = modUpdateQueue.succeeded;
    const failed = modUpdateQueue.failed;
    const lastError = modUpdateQueue.lastError;
    const profileIds = Array.from(modUpdateQueue.profileIds);
    Object.assign(modUpdateQueue, {
        done: 0, total: 0, succeeded: [], failed: [], lastError: null, currentName: null, profileIds: new Set()
    });

    // Aktivitäts-Kasten freigeben (außer eine andere Aktivität hat ihn inzwischen übernommen)
    if (sessionWidgetState.activity === 'mod-update') {
        setSessionWidgetState({ phase: 'idle', percent: null, canStop: false, activity: null });
        syncSessionWidgetWithRunningProfiles();
    }

    profileIds.forEach(profileId => {
        invalidateProfileContentCaches(profileId);
        if (currentProfile?.id === profileId && document.getElementById('profile-mods-list')) {
            loadInstalledMods(profileId, { force: true });
        }
    });
    loadInstalledModIds(true);

    if (failed.length === 0) {
        const message = succeeded.length === 1
            ? `${succeeded[0].name} aktualisiert (${succeeded[0].status.current_version || '?'} → ${succeeded[0].status.latest_version})`
            : `${succeeded.length} Mods aktualisiert`;
        showToast(escapeHtml(message), 'success', 3500);
    } else {
        const names = failed.slice(0, 3).join(', ') + (failed.length > 3 ? ` +${failed.length - 3}` : '');
        const detail = failed.length === 1 && lastError ? `: ${lastError}` : '';
        showToast(escapeHtml(`${succeeded.length} aktualisiert, ${failed.length} fehlgeschlagen (${names})${detail}`),
            succeeded.length > 0 ? 'warning' : 'error', 6000);
    }
}

function updateSingleMod(button) {
    const card = button.closest('.installed-mod-card');
    const profileId = currentProfile?.id;
    const filename = card?.dataset.filename;
    const status = profileId && filename ? getModUpdateStatus(profileId, filename) : null;
    if (!status?.latest_version_id) return;
    enqueueModUpdates(profileId, [status]);
}

function formatGameVersionList(versions) {
    const list = Array.isArray(versions) ? versions : [];
    if (list.length === 0) return 'unbekannt';
    if (list.length <= 5) return list.join(', ');
    return `${list[0]} – ${list[list.length - 1]}`;
}

function formatLoaderList(loaders) {
    return (loaders || []).map(l => SERVER_SOFTWARE[l]?.label || l).join(', ');
}

// Update-Dialog ("Updates"-Button im Profil)
async function checkForModUpdates(profileId) {
    debugLog('Checking for mod updates...', 'info');

    const profile = profiles.find(p => p.id === profileId);
    if (!profile) return;

    // Zeige Loading-Modal
    const modalHTML = `
        <div id="update-check-modal" style="position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.8); display: flex; align-items: center; justify-content: center; z-index: 10000;">
            <div style="background: var(--bg-dark); border: 2px solid var(--gold); border-radius: 10px; padding: 30px; text-align: center; min-width: 300px;">
                <div class="spinner" style="margin: 0 auto 20px;"></div>
                <h3 style="color: var(--gold); margin: 0 0 10px 0;">Suche nach Updates...</h3>
                <p style="color: var(--text-secondary); margin: 0;">Prüfe Mods auf Modrinth</p>
            </div>
        </div>
    `;
    document.body.insertAdjacentHTML('beforeend', modalHTML);

    try {
        // Läuft die Hintergrund-Prüfung noch, deren Ergebnis nutzen statt doppelt zu fragen
        await modUpdateChecksInFlight.get(profileId);

        const mods = await invoke('get_installed_mods', { profileId });
        const filenames = (mods || []).map(mod => mod.filename);
        let entry = getModUpdateEntry(profileId);
        const isFresh = !!entry && Date.now() - entry.checkedAt < MOD_UPDATE_FRESH_MS &&
            filenames.every(name => entry.results.has(name));

        if (!isFresh) {
            const startedAt = Date.now();
            await runModUpdateCheck(profileId, filenames, result => {
                storeModUpdateResults(profileId, result);
                applyModUpdateStatusesToList(profileId, result.results);
            });
            entry = modUpdateStatusCache.get(profileId);
            if (entry) entry.checkedAt = startedAt;
        }

        document.getElementById('update-check-modal')?.remove();
        updateModUpdatesButton(profileId);

        const modsByFile = new Map((mods || []).map(mod => [mod.filename, mod]));
        const results = filenames
            .map(name => entry?.results.get(name))
            .filter(Boolean)
            .map(result => ({
                ...result,
                display_name: result.mod_name || modsByFile.get(result.filename)?.name || result.filename,
                display_icon: result.icon_url || modsByFile.get(result.filename)?.icon_url || null
            }));
        const updatable = results.filter(r => r.status === 'update' && r.latest_version_id);
        // Nur Mods, deren installierte Version nicht zum Profil passt UND für die es keine passende gibt
        const incompatible = results.filter(r => r.status === 'incompatible');
        const unknownCount = results.filter(r => r.status === 'unknown').length;

        if (updatable.length === 0 && incompatible.length === 0) {
            showToast(unknownCount > 0
                ? `Alle Mods sind aktuell! (${unknownCount} nicht auf Modrinth gefunden)`
                : 'Alle Mods sind aktuell!', 'success', 3500);
            return;
        }

        modUpdateCandidates = updatable;
        selectedModUpdates = new Set(updatable.map(u => u.filename));
        currentModUpdateProfileId = profileId;

        const mcVersion = entry?.minecraftVersion || profile.minecraft_version;
        const loaderLabel = formatLoaderList([entry?.loader || profile.loader?.loader]);
        const summary = [
            `${updatable.length} updatebar`,
            incompatible.length > 0 ? `${incompatible.length} ohne passende Version` : '',
            unknownCount > 0 ? `${unknownCount} nicht auf Modrinth` : ''
        ].filter(Boolean).join(' · ');

        // Zeige Updates mit Vorauswahl
        const updateModalHTML = `
            <div id="updates-modal" style="position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.8); display: flex; align-items: center; justify-content: center; z-index: 10000;" onclick="if(event.target === this) this.remove()">
                <div style="background: var(--bg-dark); border: 2px solid var(--gold); border-radius: 10px; padding: 25px; width: min(760px, 94vw); max-height: 80vh; overflow-y: auto;" onclick="event.stopPropagation()">
                    <h3 style="color: var(--gold); margin: 0 0 6px 0;">↻ Mod-Updates</h3>
                    <p style="color: var(--text-secondary); font-size: 12px; margin: 0 0 16px 0;">
                        Minecraft ${escapeHtml(mcVersion)} · ${escapeHtml(loaderLabel)} — ${summary}
                    </p>
                    ${updatable.length > 0 ? `
                    <div style="display: flex; align-items: center; gap: 10px; margin-bottom: 10px;">
                        <label style="display: flex; align-items: center; gap: 8px; color: var(--text-secondary); font-size: 12px; cursor: pointer;">
                            <input id="select-all-update-mods" type="checkbox" checked onchange="toggleSelectAllModUpdates()" style="width: 16px; height: 16px; cursor: pointer; accent-color: var(--gold);">
                            Alle updatebaren auswählen
                        </label>
                        <div style="flex: 1;"></div>
                        <span id="updates-selected-count" style="color: var(--gold); font-size: 12px;">0 ausgewählt</span>
                    </div>
                    ` : ''}
                    <div style="display: grid; gap: 10px;">
                        ${updatable.map(u => `
                            <div style="background: var(--bg-light); border-radius: 8px; padding: 12px; display: flex; align-items: center; gap: 12px;">
                                <input type="checkbox"
                                       class="update-mod-checkbox"
                                       data-filename="${escapeAttr(u.filename)}"
                                       checked
                                       onchange="toggleModUpdateSelection(this.dataset.filename)"
                                       style="width: 17px; height: 17px; cursor: pointer; accent-color: var(--gold); flex-shrink: 0;">
                                <div style="width: 40px; height: 40px; background: var(--bg-dark); border-radius: 6px; display: flex; align-items: center; justify-content: center; overflow: hidden; flex-shrink: 0;">
                                    ${u.display_icon ? `<img alt="" src="${escapeAttr(u.display_icon)}" style="width: 100%; height: 100%; object-fit: cover;" data-fallback="icon">` : '<span style="font-size: 18px;"><i class="bi bi-box"></i></span>'}
                                </div>
                                <div style="flex: 1; min-width: 0;">
                                    <p style="margin: 0; color: var(--text-primary); font-size: 13px;">${escapeHtml(u.display_name)}</p>
                                    <p style="margin: 2px 0 0 0; color: var(--text-secondary); font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">${escapeHtml(u.filename)}</p>
                                    <p style="margin: 3px 0 0 0; font-size: 11px;">
                                        <span style="color: var(--text-secondary);">${escapeHtml(u.current_version || '?')}</span>
                                        <span style="color: var(--gold);"> → ${escapeHtml(u.latest_version || '?')}</span>
                                        ${u.current_compatible ? '' : `<span style="color: #ffb74d;" title="Die installierte Version ist für MC ${escapeAttr(formatGameVersionList(u.current_game_versions))} markiert"> · nötig für MC ${escapeHtml(mcVersion)}</span>`}
                                    </p>
                                </div>
                            </div>
                        `).join('')}
                    </div>
                    ${incompatible.length > 0 ? `
                    <h4 style="margin: 16px 0 4px 0; color: #ffb74d; font-size: 13px;">Keine passende Version für Minecraft ${escapeHtml(mcVersion)} (${escapeHtml(loaderLabel)})</h4>
                    <p style="margin: 0 0 8px 0; color: var(--text-secondary); font-size: 11px;">
                        Die installierte Version ist laut Modrinth nicht für diese Minecraft-Version markiert, und es gibt auch keine, die es ist. Sie kann trotzdem funktionieren – sonst entfernen oder ersetzen.
                    </p>
                    <div style="display: grid; gap: 8px;">
                        ${incompatible.map(u => `
                            <div style="background: rgba(255, 183, 77, 0.1); border: 1px solid rgba(255, 183, 77, 0.35); border-radius: 8px; padding: 10px; display: flex; align-items: start; gap: 10px;">
                                <div style="font-size: 15px; color: #ffb74d; margin-top: 1px;"><i class="bi bi-exclamation-triangle-fill"></i></div>
                                <div style="flex: 1; min-width: 0;">
                                    <p style="margin: 0; color: var(--text-primary); font-size: 13px;">${escapeHtml(u.display_name)}</p>
                                    <p style="margin: 2px 0 0 0; color: var(--text-secondary); font-size: 11px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">${escapeHtml(u.filename)}</p>
                                    <p style="margin: 4px 0 0 0; color: var(--text-secondary); font-size: 11px;">
                                        Installiert: <span style="color: #ffb74d;">${escapeHtml(u.current_version || '?')}</span>
                                        für MC ${escapeHtml(formatGameVersionList(u.current_game_versions))}${u.current_loaders?.length ? ` (${escapeHtml(formatLoaderList(u.current_loaders))})` : ''}
                                    </p>
                                </div>
                            </div>
                        `).join('')}
                    </div>` : ''}
                    <div style="display: grid; grid-template-columns: 1fr 1fr; gap: 10px; margin-top: 16px;">
                        <button id="update-selected-btn" class="btn" onclick="applySelectedModUpdates()" style="padding: 10px;" ${updatable.length === 0 ? 'disabled' : ''}>
                            Ausgewählte updaten
                        </button>
                        <button class="btn btn-secondary" onclick="document.getElementById('updates-modal').remove()" style="padding: 10px;">
                            Schließen
                        </button>
                    </div>
                </div>
            </div>
        `;
        document.body.insertAdjacentHTML('beforeend', updateModalHTML);
        refreshModUpdateSelectionUi();

    } catch (error) {
        document.getElementById('update-check-modal')?.remove();
        debugLog('Failed to check updates: ' + error, 'error');
        showToast('Fehler beim Prüfen auf Updates: ' + error, 'error', 5000);
    }
}

function toggleModUpdateSelection(filename) {
    if (selectedModUpdates.has(filename)) {
        selectedModUpdates.delete(filename);
    } else {
        selectedModUpdates.add(filename);
    }
    refreshModUpdateSelectionUi();
}

function toggleSelectAllModUpdates() {
    const selectAll = document.getElementById('select-all-update-mods');
    const checkboxes = document.querySelectorAll('.update-mod-checkbox');
    if (!selectAll) return;

    if (selectAll.checked) {
        checkboxes.forEach(cb => {
            cb.checked = true;
            selectedModUpdates.add(cb.dataset.filename);
        });
    } else {
        checkboxes.forEach(cb => {
            cb.checked = false;
            selectedModUpdates.delete(cb.dataset.filename);
        });
    }

    refreshModUpdateSelectionUi();
}

function refreshModUpdateSelectionUi() {
    const selectedCount = document.getElementById('updates-selected-count');
    const updateBtn = document.getElementById('update-selected-btn');
    const selectAll = document.getElementById('select-all-update-mods');
    const total = modUpdateCandidates.length;
    const selected = selectedModUpdates.size;

    if (selectedCount) selectedCount.textContent = `${selected} ausgewählt`;

    if (updateBtn) {
        updateBtn.disabled = selected === 0;
        updateBtn.textContent = selected > 0
            ? `Ausgewählte updaten (${selected})`
            : 'Ausgewählte updaten';
    }

    if (selectAll) {
        selectAll.checked = total > 0 && selected === total;
        selectAll.indeterminate = selected > 0 && selected < total;
    }
}

function applySelectedModUpdates() {
    if (!currentModUpdateProfileId) return;
    if (selectedModUpdates.size === 0) {
        showToast('Bitte wähle zuerst Mods aus', 'warning', 2500);
        return;
    }

    const profileId = currentModUpdateProfileId;
    const selectedUpdates = modUpdateCandidates.filter(u =>
        selectedModUpdates.has(u.filename) && u.latest_version_id
    );
    if (selectedUpdates.length === 0) {
        showToast('Keine gültigen Updates ausgewählt', 'warning', 2500);
        return;
    }

    document.getElementById('updates-modal')?.remove();
    selectedModUpdates.clear();
    modUpdateCandidates = [];
    currentModUpdateProfileId = null;

    // Fortschritt läuft über den Aktivitäts-Kasten oben rechts
    enqueueModUpdates(profileId, selectedUpdates);
}

async function toggleMod(profileId, filename, isCurrentlyDisabled) {
    debugLog('Toggling mod: ' + filename + ' (currently disabled: ' + isCurrentlyDisabled + ')', 'info');

    try {
        await invoke('toggle_mod', {
            profileId,
            filename,
            enable: isCurrentlyDisabled  // Wenn deaktiviert, dann aktivieren und umgekehrt
        });

        debugLog('Mod toggled successfully', 'success');
        invalidateProfileContentCaches(profileId);
        loadInstalledMods(profileId, { force: true });

        // Toast-Benachrichtigung
        showToast(`Mod ${isCurrentlyDisabled ? 'aktiviert' : 'deaktiviert'}!`, 'success', 3000);

    } catch (error) {
        debugLog('Failed to toggle mod: ' + error, 'error');
        showToast('Fehler beim Umschalten der Mod: ' + error, 'error', 3000);
    }
}

async function deleteMod(profileId, filename) {

    debugLog('Deleting mod: ' + filename, 'info');

    try {
        await invoke('delete_mod', { profileId, filename });

        debugLog('Mod deleted successfully', 'success');
        await loadInstalledModIds(true); // Cache aktualisieren
        invalidateProfileContentCaches(profileId);
        loadInstalledMods(profileId, { force: true });

        // Toast-Benachrichtigung
        showToast(`Mod "${filename}" wurde gelöscht!`, 'success', 3000);

    } catch (error) {
        debugLog('Failed to delete mod: ' + error, 'error');
        showToast('Fehler beim Löschen der Mod: ' + error, 'error', 3000);
    }
}

async function openModsFolder(profileId) {
    debugLog('Opening mods folder for: ' + profileId, 'info');
    try {
        await invoke('open_profile_folder', { profileId, subfolder: 'mods' });
    } catch (error) {
        debugLog('Failed to open mods folder: ' + error, 'error');
        showToast('Konnte Mods-Ordner nicht öffnen: ' + error, 'error', 5000);
    }
}

async function openResourcePacksFolder(profileId) {
    debugLog('Opening resourcepacks folder', 'info');
    try {
        await invoke('open_profile_folder', { profileId, subfolder: 'resourcepacks' });
    } catch (error) {
        debugLog('Failed to open folder: ' + error, 'error');
    }
}

async function openShaderPacksFolder(profileId) {
    debugLog('Opening shaderpacks folder', 'info');
    try {
        await invoke('open_profile_folder', { profileId, subfolder: 'shaderpacks' });
    } catch (error) {
        debugLog('Failed to open folder: ' + error, 'error');
    }
}

// ==================== RESOURCE PACKS ====================

async function loadInstalledResourcePacks(profileId, options = {}) {
    const list = document.getElementById('profile-resourcepacks-list');
    if (!list) return;

    const forceReload = !!options.force;
    const cached = profileResourcePacksCache.get(profileId);
    if (!forceReload && Array.isArray(cached)) {
        renderInstalledResourcePacks(cached, profileId);
        return;
    }

    try {
        const packs = await invoke('get_installed_resourcepacks', { profileId });
        const normalized = packs || [];
        profileResourcePacksCache.set(profileId, normalized);
        installedResourcePackNamesProfileId = null;
        installedResourcePackNames.clear();
        renderInstalledResourcePacks(normalized, profileId);
    } catch (error) {
        debugLog('Failed to load resource packs: ' + error, 'error');
        list.innerHTML = `
            <div style="text-align: center; padding: 40px; color: #f44336;">
                Fehler beim Laden: ${error}
            </div>
        `;
    }
}

function renderInstalledResourcePacks(packs, profileId) {
    const list = document.getElementById('profile-resourcepacks-list');
    if (!list) return;

    if (!packs || packs.length === 0) {
        list.innerHTML = `
            <div style="text-align: center; padding: 60px 20px; color: var(--text-secondary);">
                <div style="font-size: 48px; margin-bottom: 15px;"><i class="bi bi-palette"></i></div>
                <p>Keine Resource Packs installiert</p>
                <p style="font-size: 14px; margin-top: 10px;">
                    Klicke auf "+ Resource Pack" um Packs zu durchsuchen
                </p>
            </div>
        `;
        return;
    }

    list.innerHTML = packs.map(pack => {
        const sizeStr = pack.size > 0 ? `${(pack.size / 1024 / 1024).toFixed(2)} MB` : '';
        const iconSrc = resolvePackIconSrc(pack);
        const iconHTML = iconSrc
            ? `<img alt="" src="${escapeAttr(iconSrc)}" style="width: 48px; height: 48px; border-radius: 4px; object-fit: cover;" data-fallback="sibling">
               <div style="display: none; width: 48px; height: 48px; font-size: 32px; align-items: center; justify-content: center;"><i class="bi bi-palette"></i></div>`
            : `<div style="font-size: 32px;"><i class="bi bi-palette"></i></div>`;

        return `
            <div style="background: var(--bg-light); padding: 12px; border-radius: 8px; display: flex; align-items: center; gap: 15px;">
                <div style="width: 48px; height: 48px; display: flex; align-items: center; justify-content: center; flex-shrink: 0; overflow: hidden; border-radius: 4px; background: var(--bg-dark);">
                    ${iconHTML}
                </div>
                <div style="flex: 1; min-width: 0;">
                    <div style="color: var(--text-primary); font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">
                        ${pack.name}
                    </div>
                    <div style="color: var(--text-secondary); font-size: 11px;">
                        ${pack.is_folder ? '<i class="bi bi-folder"></i> Ordner' : '· ' + sizeStr}
                    </div>
                </div>
                <button class="btn btn-secondary content-action-btn content-action-delete" onclick="deleteResourcePack('${profileId}', '${pack.name.replace(/'/g, "\\'")}')"
                        title="Löschen">
                    <i class="bi bi-trash"></i>
                </button>
            </div>
        `;
    }).join('');
}

async function refreshResourcePacks(profileId) {
    invalidateProfileContentCaches(profileId);
    await loadInstalledResourcePacks(profileId, { force: true });
    showToast('Resource Packs aktualisiert', 'success', 2000);
}

async function deleteResourcePack(profileId, name) {
    debugLog('Deleting resource pack: ' + name, 'info');

    try {
        await invoke('delete_resourcepack', { profileId, name });

        debugLog('Resource Pack deleted successfully', 'success');
        invalidateProfileContentCaches(profileId);
        await loadInstalledResourcePackNames(true);
        await loadInstalledResourcePacks(profileId, { force: true });

        showToast(`Resource Pack "${name}" wurde gelöscht!`, 'success', 3000);

    } catch (error) {
        debugLog('Failed to delete resource pack: ' + error, 'error');
        showToast('Fehler beim Löschen: ' + error, 'error', 3000);
    }
}

function browseResourcePacks(profileId) {
    // Speichere aktuelles Profil und wechsle zum Content Browser
    const profile = profiles.find(p => p.id === profileId);
    if (profile) {
        currentProfile = profile;
        openedFromProfile = true; // Wichtig! Sonst werden Filter zurückgesetzt
    }

    // Wechsle zu Resource Packs
    switchPage('mods');
    switchContentType('resourcepacks', { force: true });
}

function openContentBrowser(profileId) {
    // Speichere aktuelles Profil und setze Flag VOR switchPage
    const profile = profiles.find(p => p.id === profileId);
    if (profile) {
        recordNavigation({ type: 'browser', context: 'profile', profileId });
        currentProfile = profile;
        openedFromProfile = true; // Setze Flag BEVOR switchPage aufgerufen wird
        debugLog('Opening Content Browser from profile: ' + profile.name + ', SubTab: ' + currentProfileSubTab, 'info');
    }

    // Wechsle zum Content Browser
    switchPage('mods');
    if (profile) browserViewKey = `browser:profile:${profileId}`;

    // Setze Filter ZUERST (bevor Content geladen wird!)
    if (currentProfile) {
        // Setze nur selectedFilters, OHNE DOM zu ändern (das passiert später)
        selectedFilters.version = currentProfile.minecraft_version;

        // Loader nur für Mods/Modpacks
        if (currentProfileSubTab === 'mods' || currentProfileSubTab === 'modpacks') {
            const loaderName = currentProfile.loader.loader;
            if (loaderName && loaderName !== 'vanilla') {
                selectedFilters.loader = loaderName;
            } else {
                selectedFilters.loader = '';
            }
        } else {
            selectedFilters.loader = ''; // Kein Loader für Resource Packs/Shader Packs
        }

        debugLog('Pre-set filters: version=' + selectedFilters.version + ', loader=' + selectedFilters.loader, 'info');
    }

    // JETZT wechsle zum richtigen Content Type (mit bereits gesetzten Filtern!)
    switchContentType(currentProfileSubTab, { force: true });

    debugLog('Content Browser opened: currentContentType = ' + currentContentType, 'info');

    // Update DOM-Elemente (Dropdowns etc.) nach dem Content geladen wurde
    setTimeout(() => {
        if (currentProfile) {
            // Update Version Dropdown
            const versionFilter = document.getElementById('filter-version');
            if (versionFilter && currentProfile.minecraft_version) {
                versionFilter.value = currentProfile.minecraft_version;
            }

            // Update Loader Dropdown (nur für Mods/Modpacks)
            if (currentContentType === 'mods' || currentContentType === 'modpacks') {
                const loaderName = currentProfile.loader.loader;
                const loaderSelect = document.getElementById('filter-loader');
                if (loaderSelect && loaderName && loaderName !== 'vanilla') {
                    loaderSelect.value = loaderName;
                } else if (loaderSelect) {
                    loaderSelect.value = '';
                }
            }
        }

        // Verstecke Modpacks Button wenn aus Profil geöffnet
        const modpacksBtn = document.querySelector('[data-content-type="modpacks"]');
        if (modpacksBtn && currentProfile) {
            modpacksBtn.style.display = 'none';
        }
    }, 50);
}

// ==================== SHADER PACKS (Profil) ====================

async function loadInstalledShaderPacks(profileId, options = {}) {
    const list = document.getElementById('profile-shaderpacks-list');
    if (!list) return;

    const forceReload = !!options.force;
    const cached = profileShaderPacksCache.get(profileId);
    if (!forceReload && Array.isArray(cached)) {
        renderInstalledShaderPacks(cached, profileId);
        return;
    }

    try {
        const packs = await invoke('get_installed_shaderpacks', { profileId });
        const normalized = packs || [];
        profileShaderPacksCache.set(profileId, normalized);
        installedShaderPackNamesProfileId = null;
        installedShaderPackNames.clear();
        renderInstalledShaderPacks(normalized, profileId);
    } catch (error) {
        debugLog('Failed to load shader packs: ' + error, 'error');
        list.innerHTML = `
            <div style="text-align: center; padding: 40px; color: #f44336;">
                Fehler beim Laden: ${error}
            </div>
        `;
    }
}

function renderInstalledShaderPacks(packs, profileId) {
    const list = document.getElementById('profile-shaderpacks-list');
    if (!list) return;

    if (!packs || packs.length === 0) {
        list.innerHTML = `
            <div style="text-align: center; padding: 60px 20px; color: var(--text-secondary);">
                <div style="font-size: 48px; margin-bottom: 15px;"><i class="bi bi-stars"></i></div>
                <p>Keine Shader Packs installiert</p>
                <p style="font-size: 14px; margin-top: 10px;">
                    Benötigt Iris oder OptiFine<br>
                    Klicke auf "+ Add Content" um Shader zu durchsuchen
                </p>
            </div>
        `;
        return;
    }

    list.innerHTML = packs.map(pack => {
        const sizeStr = pack.size > 0 ? `${(pack.size / 1024 / 1024).toFixed(2)} MB` : '';
        const iconSrc = resolvePackIconSrc(pack);
        const iconHTML = iconSrc
            ? `<img alt="" src="${escapeAttr(iconSrc)}" style="width: 48px; height: 48px; border-radius: 4px; object-fit: cover;" data-fallback="sibling">
               <div style="display: none; width: 48px; height: 48px; font-size: 32px; align-items: center; justify-content: center;"><i class="bi bi-stars"></i></div>`
            : `<i class="bi bi-stars"></i>`;

        return `
            <div style="background: var(--bg-light); padding: 12px; border-radius: 8px; display: flex; align-items: center; gap: 15px;">
                <div style="width: 48px; height: 48px; display: flex; align-items: center; justify-content: center; flex-shrink: 0; font-size: 32px; border-radius: 4px; overflow: hidden; background: var(--bg-dark);">
                    ${iconHTML}
                </div>
                <div style="flex: 1; min-width: 0;">
                    <div style="color: var(--text-primary); font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">
                        ${pack.name}
                    </div>
                    <div style="color: var(--text-secondary); font-size: 11px;">
                        ${pack.is_folder ? '<i class="bi bi-folder"></i> Ordner' : '· ' + sizeStr}
                    </div>
                </div>
                <button class="btn btn-secondary content-action-btn content-action-delete" onclick="deleteShaderPack('${profileId}', '${pack.name.replace(/'/g, "\\'")}')"
                        title="Löschen">
                    <i class="bi bi-trash"></i>
                </button>
            </div>
        `;
    }).join('');
}

async function refreshShaderPacks(profileId) {
    invalidateProfileContentCaches(profileId);
    await loadInstalledShaderPacks(profileId, { force: true });
    showToast('Shader Packs aktualisiert', 'success', 2000);
}

async function deleteShaderPack(profileId, name) {
    debugLog('Deleting shader pack: ' + name, 'info');

    try {
        await invoke('delete_shaderpack', { profileId, name });

        debugLog('Shader Pack deleted successfully', 'success');
        invalidateProfileContentCaches(profileId);
        await loadInstalledShaderPackNames(true);
        await loadInstalledShaderPacks(profileId, { force: true });

        showToast(`Shader Pack "${name}" wurde gelöscht!`, 'success', 3000);

    } catch (error) {
        debugLog('Failed to delete shader pack: ' + error, 'error');
        showToast('Fehler beim Löschen: ' + error, 'error', 3000);
    }
}

// ==================== WORLDS ====================

async function loadWorlds(profileId) {
    const list = document.getElementById('profile-worlds-list');
    if (!list) return;

    try {
        const worlds = await invoke('get_worlds', { profileId });

        if (worlds.length === 0) {
            list.innerHTML = `
                <div style="text-align: center; padding: 60px 20px; color: var(--text-secondary);">
                    <div style="font-size: 48px; margin-bottom: 15px;"><i class="bi bi-globe"></i></div>
                    <p>Keine Welten gefunden</p>
                    <p style="font-size: 14px; margin-top: 10px;">
                        Starte Minecraft und erstelle eine Welt
                    </p>
                </div>
            `;
            return;
        }

        list.innerHTML = worlds.map(world => {
            // Formatiere letzte Spielzeit
            const lastPlayed = world.last_played > 0
                ? formatTimestamp(world.last_played)
                : 'Unbekannt';

            // Formatiere Größe
            const sizeStr = formatBytes(world.size_bytes);

            // Icon oder Fallback
            const iconHtml = world.icon_base64
                ? `<img alt="" src="${world.icon_base64}" style="width: 100%; height: 100%; object-fit: cover; border-radius: 4px;">`
                : `<div style="font-size: 32px;"><i class="bi bi-globe"></i></div>`;

            return `
                <div style="background: var(--bg-light); padding: 12px; border-radius: 8px; display: flex; align-items: center; gap: 15px;">
                    <div style="width: 48px; height: 48px; display: flex; align-items: center; justify-content: center; flex-shrink: 0; background: var(--bg-dark); border-radius: 4px; overflow: hidden;">
                        ${iconHtml}
                    </div>
                    <div style="flex: 1; min-width: 0;">
                        <div style="color: var(--text-primary); font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">
                            ${world.name}
                        </div>
                        <div style="color: var(--text-secondary); font-size: 11px; display: flex; gap: 10px; flex-wrap: wrap;">
                            <span><i class="bi bi-controller"></i> ${world.game_mode}</span>
                            <span><i class="bi bi-calendar3"></i> ${lastPlayed}</span>
                            <span><i class="bi bi-box"></i> ${sizeStr}</span>
                        </div>
                    </div>
                    <button class="btn btn-gold" onclick="launchWorld('${profileId}', '${world.folder_name}')" 
                            style="padding: 8px 16px; font-size: 12px;">
                        <i class="bi bi-play-fill"></i> Play
                    </button>
                </div>
            `;
        }).join('');

    } catch (error) {
        debugLog('Failed to load worlds: ' + error, 'error');
        list.innerHTML = `
            <div style="text-align: center; padding: 40px; color: #f44336;">
                Fehler beim Laden: ${error}
            </div>
        `;
    }
}

async function refreshWorlds(profileId) {
    await loadWorlds(profileId);
    showToast('Welten aktualisiert', 'success', 2000);
}

async function openWorldsFolder(profileId) {
    try {
        await invoke('open_profile_folder', { profileId: profileId, subfolder: 'saves' });
        showToast('Welten-Ordner wird geöffnet...', 'info', 2000);
    } catch (error) {
        debugLog('Failed to open worlds folder: ' + error, 'error');
        showToast('Fehler beim Öffnen: ' + error, 'error', 3000);
    }
}

async function launchWorld(profileId, worldName) {
    try {
        showToast(`Starte Welt "${worldName}"...`, 'info', 3000);
        await invoke('launch_world', { profileId, worldName });
        showToast(`Minecraft startet mit Welt "${worldName}"`, 'success', 3000);
    } catch (error) {
        debugLog('Failed to launch world: ' + error, 'error');
        showToast('Fehler beim Starten: ' + error, 'error', 5000);
    }
}

// ==================== SERVERS ====================

async function loadServers(profileId) {
    const list = document.getElementById('profile-servers-list');
    if (!list) return;

    // Zeige Loading-Spinner sofort
    list.innerHTML = `
        <div style="text-align: center; padding: 40px; color: var(--text-secondary);">
            <div class="spinner" style="margin: 0 auto 15px;"></div>
            <p>Lade Server & prüfe Status...</p>
        </div>
    `;

    try {
        const servers = await invoke('get_servers', { profileId });

        if (servers.length === 0) {
            list.innerHTML = `
                <div style="text-align: center; padding: 60px 20px; color: var(--text-secondary);">
                    <div style="font-size: 48px; margin-bottom: 15px;"><i class="bi bi-display"></i></div>
                    <p>Keine Server gespeichert</p>
                    <p style="font-size: 14px; margin-top: 10px;">
                        Klicke auf <b>"+ Server hinzufügen"</b> um einen Server einzutragen
                    </p>
                </div>
            `;
            return;
        }

        list.innerHTML = servers.map(server => {
            // Icon oder Fallback
            const iconHtml = server.icon_base64
                ? `<img alt="" src="${server.icon_base64}" style="width: 100%; height: 100%; object-fit: cover; border-radius: 4px; image-rendering: pixelated;">`
                : `<div style="font-size: 28px; color: var(--text-secondary);"><i class="bi bi-display"></i></div>`;

            // Spieleranzahl (grün, rechts)
            const playerInfo = (server.online_players != null && server.max_players != null)
                ? `<span style="color: #55FF55; font-family: 'Minecraft', monospace; font-size: 12px; white-space: nowrap; flex-shrink: 0;">${server.online_players}/${server.max_players}</span>`
                : '';

            // MOTD: HTML-Version mit Minecraft-Farben oder Fallback auf clean/IP
            let motdHtml;
            if (server.motd_html && server.motd_html.length > 0) {
                // Jede Zeile als eigene Zeile rendern (wie in Minecraft)
                motdHtml = server.motd_html.map(line =>
                    `<div style="white-space: pre; overflow: hidden; text-overflow: ellipsis; line-height: 1.4; text-align: center;">${line}</div>`
                ).join('');
            } else if (server.motd) {
                motdHtml = `<div style="color: #AAAAAA;">${server.motd}</div>`;
            } else {
                motdHtml = `<div style="color: #555555;">${server.ip}</div>`;
            }

            // Escape für onclick
            const escapedIp = server.ip.replace(/'/g, "\\'");
            const escapedName = server.name.replace(/'/g, "\\'");

            return `
                <div style="background: var(--bg-light); padding: 10px; border-radius: 8px; display: flex; align-items: center; gap: 10px; transition: all 0.2s; border: 2px solid transparent;"
                     onmouseover="this.style.borderColor='var(--gold)'" onmouseout="this.style.borderColor='transparent'">
                    <div style="flex: 1; min-width: 0; background: #000000cc; border-radius: 6px; padding: 10px 14px; display: flex; gap: 12px; align-items: center;">
                        <div style="width: 64px; height: 64px; display: flex; align-items: center; justify-content: center; flex-shrink: 0; background: #111; border-radius: 4px; overflow: hidden;">
                            ${iconHtml}
                        </div>
                        <div style="flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 4px;">
                            <div style="color: #FFFFFF; font-family: 'Minecraft', monospace; font-size: 20px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">
                                ${server.name}
                            </div>
                            <div style="font-family: 'Minecraft', monospace; font-size: 16px; text-align: center;">
                                ${motdHtml}
                            </div>
                        </div>
                    </div>
                    ${playerInfo ? `<div style="flex-shrink: 0; padding: 0 4px;">${playerInfo}</div>` : ''}
                    <div style="display: flex; flex-direction: column; gap: 4px; flex-shrink: 0;">
                        <button class="btn" onclick="launchServer('${profileId}', '${escapedIp}')" 
                                style="padding: 8px 16px; font-size: 12px; border-radius: 6px;">
                            <i class="bi bi-play-fill"></i> Join
                        </button>
                        <button class="btn btn-secondary" onclick="removeServer('${profileId}', '${escapedIp}', '${escapedName}')" 
                                style="padding: 6px 10px; font-size: 11px; border-radius: 6px; color: #f44336;" title="Server entfernen">
                            <i class="bi bi-trash"></i>
                        </button>
                    </div>
                </div>
            `;
        }).join('');

    } catch (error) {
        debugLog('Failed to load servers: ' + error, 'error');
        list.innerHTML = `
            <div style="text-align: center; padding: 40px; color: #f44336;">
                Fehler beim Laden: ${error}
            </div>
        `;
    }
}

async function refreshServers(profileId) {
    await loadServers(profileId);
    showToast('Server aktualisiert', 'success', 2000);
}

function showAddServerModal() {
    const modal = document.getElementById('add-server-modal');
    if (modal) {
        modal.classList.add('active');
        // Focus auf erstes Feld
        setTimeout(() => {
            const nameInput = document.getElementById('add-server-name');
            if (nameInput) nameInput.focus();
        }, 100);
    }
}

function hideAddServerModal() {
    const modal = document.getElementById('add-server-modal');
    if (modal) {
        modal.classList.remove('active');
        // Felder zurücksetzen
        const nameInput = document.getElementById('add-server-name');
        const ipInput = document.getElementById('add-server-ip');
        if (nameInput) nameInput.value = '';
        if (ipInput) ipInput.value = '';
    }
}

async function submitAddServer(profileId) {
    const nameInput = document.getElementById('add-server-name');
    const ipInput = document.getElementById('add-server-ip');

    const name = nameInput ? nameInput.value.trim() : '';
    const ip = ipInput ? ipInput.value.trim() : '';

    if (!name) {
        showToast('Bitte gib einen Servernamen ein', 'error', 3000);
        if (nameInput) nameInput.focus();
        return;
    }
    if (!ip) {
        showToast('Bitte gib eine Server-Adresse ein', 'error', 3000);
        if (ipInput) ipInput.focus();
        return;
    }

    try {
        showToast(`Server "${name}" wird hinzugefügt...`, 'info', 2000);
        await invoke('add_server', { profileId, name, ip });
        showToast(`Server "${name}" hinzugefügt!`, 'success', 3000);
        hideAddServerModal();
        await loadServers(profileId);
    } catch (error) {
        debugLog('Failed to add server: ' + error, 'error');
        showToast('Fehler: ' + error, 'error', 5000);
    }
}

async function removeServer(profileId, serverIp, serverName) {
    const confirmed = await showConfirmModal({
        title: 'Server entfernen',
        message: 'Server ',
        highlight: `${serverName} (${serverIp})`,
        suffix: ' wirklich entfernen?',
        confirmText: 'Entfernen'
    });
    if (!confirmed) return;

    try {
        await invoke('remove_server', { profileId, ip: serverIp });
        showToast(`Server "${serverName}" entfernt`, 'success', 3000);
        await loadServers(profileId);
    } catch (error) {
        debugLog('Failed to remove server: ' + error, 'error');
        showToast('Fehler beim Entfernen: ' + error, 'error', 5000);
    }
}

async function launchServer(profileId, serverIp) {
    try {
        showToast(`Verbinde zu Server "${serverIp}"...`, 'info', 3000);
        await invoke('launch_server', { profileId, serverIp });
        showToast(`Minecraft startet und verbindet zu "${serverIp}"`, 'success', 3000);
    } catch (error) {
        debugLog('Failed to launch server: ' + error, 'error');
        showToast('Fehler beim Verbinden: ' + error, 'error', 5000);
    }
}

/// ==================== LOCAL SERVER HUB ====================

const SERVER_SOFTWARE = {
    paper: { label: 'Paper', icon: 'bi-send-fill', hint: 'Schnell und stabil – empfohlen für Plugin-Server.', versionHint: 'latest oder Build-Nummer' },
    purpur: { label: 'Purpur', icon: 'bi-stars', hint: 'Paper-Fork mit vielen zusätzlichen Gameplay-Optionen.', versionHint: 'latest oder Build-Nummer' },
    folia: { label: 'Folia', icon: 'bi-diagram-3-fill', hint: 'Multithreaded Paper-Fork für sehr große Server. Viele Plugins sind nicht Folia-kompatibel.', versionHint: 'latest oder Build-Nummer' },
    spigot: { label: 'Spigot', icon: 'bi-droplet-fill', hint: 'Wird beim ersten Start mit den offiziellen BuildTools gebaut (dauert 5–10 Minuten).', versionHint: null },
    bukkit: { label: 'Bukkit', icon: 'bi-bucket-fill', hint: 'CraftBukkit – wird beim ersten Start mit den BuildTools gebaut (dauert 5–10 Minuten).', versionHint: null },
    vanilla: { label: 'Vanilla', icon: 'bi-box-fill', hint: 'Offizieller Mojang-Server ohne Plugins oder Mods.', versionHint: null },
    fabric: { label: 'Fabric', icon: 'bi-grid-3x3-gap-fill', hint: 'Leichtgewichtiger Loader – empfohlen für Mod-Server.', versionHint: 'latest oder Loader-Version (z.B. 0.16.9)' },
    quilt: { label: 'Quilt', icon: 'bi-grid-fill', hint: 'Fabric-kompatibler Loader. Fabric-Mods funktionieren meistens auch.', versionHint: 'latest oder Loader-Version' },
    forge: { label: 'Forge', icon: 'bi-hammer', hint: 'Klassischer Mod-Loader mit der größten Auswahl an älteren Mods.', versionHint: 'latest oder Forge-Version (z.B. 47.3.0)' },
    neoforge: { label: 'NeoForge', icon: 'bi-fire', hint: 'Moderner Forge-Nachfolger (ab Minecraft 1.20.1).', versionHint: 'latest oder NeoForge-Version (z.B. 21.1.77)' },
};

const SERVER_SOFTWARE_BY_MODE = {
    plugins: ['paper', 'purpur', 'folia', 'spigot', 'bukkit', 'vanilla'],
    modded: ['fabric', 'quilt', 'forge', 'neoforge', 'vanilla'],
};

const SERVER_METRIC_WINDOW_MS = 120000;
// Die Zeitachse läuft etwas hinter "jetzt", damit neue Messwerte weich von rechts einlaufen
const SERVER_METRIC_RENDER_DELAY_MS = 1500;
const serverMetricHover = { cpu: null, ram: null, net: null };
let serverMetricAnimationFrame = null;
let serverMetricLastRuntime = null;
let serverBackupBusy = false;
let serverInstalledContentCache = [];
const selectedServerContent = new Set();
let serverInstalledProjectIds = new Set();
let serverInstalledProjectIdsServerId = null;
let serverBrowserContext = null;
let serverBrowserNavigation = false;
let pendingServerDetailOpen = null;
const browserModeBackup = { loaderOptions: null, modsBtnText: null };

/**
 * Bestätigungsfenster im Stil des "Profil löschen"-Dialogs.
 * `message` wird als Text eingefügt; `highlight` wird hervorgehoben dargestellt.
 */
/** Eingabe-Dialog im Launcher-Stil (Ersatz für das native prompt()). Liefert den Text oder null. */
function showPromptModal({ title, message = '', defaultValue = '', confirmText = 'OK', cancelText = 'Cancel', icon = 'bi-pencil' }) {
    return new Promise((resolve) => {
        closeDynamicModal('lion-prompt-modal');

        const modal = document.createElement('div');
        modal.id = 'lion-prompt-modal';
        modal.style.cssText = 'position: fixed; inset: 0; background: rgba(0,0,0,0.82); display: flex; align-items: center; justify-content: center; z-index: 10002;';
        modal.innerHTML = `
            <div style="background: var(--bg-dark); border: 2px solid var(--gold); border-radius: 12px; width: min(450px, 90vw); padding: 20px;" onclick="event.stopPropagation()">
                <h3 style="margin: 0 0 10px 0; color: var(--gold); font-size: 20px;"><i class="bi ${icon}"></i> <span data-prompt-title></span></h3>
                <p data-prompt-message style="margin: 0 0 10px 0; color: var(--text-secondary); font-size: 13px; line-height: 1.5;"></p>
                <input type="text" data-prompt-input
                       style="width: 100%; box-sizing: border-box; padding: 9px 10px; margin-bottom: 16px; background: var(--bg-light);
                              border: 1px solid var(--bg-medium); border-radius: 6px; color: var(--text-primary); font-size: 13px; outline: none;">
                <div style="display: flex; justify-content: flex-end; gap: 10px;">
                    <button class="btn btn-secondary" data-prompt-cancel></button>
                    <button class="btn" data-prompt-ok></button>
                </div>
            </div>
        `;

        modal.querySelector('[data-prompt-title]').textContent = title;
        modal.querySelector('[data-prompt-message]').textContent = message;
        modal.querySelector('[data-prompt-cancel]').textContent = cancelText;
        modal.querySelector('[data-prompt-ok]').textContent = confirmText;
        const input = modal.querySelector('[data-prompt-input]');
        input.value = defaultValue;

        const onKey = (event) => {
            if (event.key === 'Escape') cleanup(null);
            if (event.key === 'Enter') cleanup(input.value);
        };
        const cleanup = (result) => {
            document.removeEventListener('keydown', onKey);
            if (modal.parentElement) modal.remove();
            resolve(result);
        };

        modal.addEventListener('click', () => cleanup(null));
        modal.querySelector('[data-prompt-cancel]').addEventListener('click', () => cleanup(null));
        modal.querySelector('[data-prompt-ok]').addEventListener('click', () => cleanup(input.value));
        document.addEventListener('keydown', onKey);
        document.body.appendChild(modal);
        input.focus();
        input.select();
    });
}

function showConfirmModal({ title, message, highlight = '', suffix = '?', details = '', confirmText = 'Yes I will', cancelText = 'Cancel', icon = 'bi-trash', danger = true }) {
    return new Promise((resolve) => {
        closeDynamicModal('lion-confirm-modal');
        const color = danger ? '#f44336' : 'var(--gold)';

        const modal = document.createElement('div');
        modal.id = 'lion-confirm-modal';
        modal.style.cssText = 'position: fixed; inset: 0; background: rgba(0,0,0,0.82); display: flex; align-items: center; justify-content: center; z-index: 10002;';
        modal.innerHTML = `
            <div style="background: var(--bg-dark); border: 2px solid ${color}; border-radius: 12px; width: min(450px, 90vw); padding: 20px;" onclick="event.stopPropagation()">
                <h3 style="margin: 0 0 10px 0; color: ${color}; font-size: 20px;"><i class="bi ${icon}"></i> <span data-confirm-title></span></h3>
                <p style="margin: 0 0 ${details ? '8px' : '16px'} 0; color: var(--text-secondary); font-size: 13px; line-height: 1.5;">
                    <span data-confirm-message></span>
                    ${highlight ? '<strong data-confirm-highlight style="color: var(--text-primary); word-break: break-word;"></strong><span data-confirm-suffix></span>' : ''}
                </p>
                ${details ? '<p data-confirm-details style="margin: 0 0 16px 0; color: var(--text-secondary); font-size: 12px; line-height: 1.5;"></p>' : ''}
                <div style="display: flex; justify-content: flex-end; gap: 10px;">
                    <button class="btn btn-secondary" data-confirm-cancel></button>
                    <button class="btn" data-confirm-ok style="${danger ? 'background:#f44336; color:#fff;' : ''}"></button>
                </div>
            </div>
        `;

        modal.querySelector('[data-confirm-title]').textContent = title;
        modal.querySelector('[data-confirm-message]').textContent = message;
        if (highlight) {
            modal.querySelector('[data-confirm-highlight]').textContent = highlight;
            modal.querySelector('[data-confirm-suffix]').textContent = suffix;
        }
        if (details) modal.querySelector('[data-confirm-details]').textContent = details;
        modal.querySelector('[data-confirm-cancel]').textContent = cancelText;
        modal.querySelector('[data-confirm-ok]').textContent = confirmText;

        const onKey = (event) => {
            if (event.key === 'Escape') cleanup(false);
        };
        const cleanup = (result) => {
            document.removeEventListener('keydown', onKey);
            if (modal.parentElement) modal.remove();
            resolve(result);
        };

        modal.addEventListener('click', () => cleanup(false));
        modal.querySelector('[data-confirm-cancel]').addEventListener('click', () => cleanup(false));
        modal.querySelector('[data-confirm-ok]').addEventListener('click', () => cleanup(true));
        document.addEventListener('keydown', onKey);
        document.body.appendChild(modal);
        modal.querySelector('[data-confirm-cancel]').focus();
    });
}

function getServerModeLabel(mode) {
    return mode === 'plugins' ? 'Plugins' : 'Modded';
}

function getServerSoftwareLabel(software) {
    return SERVER_SOFTWARE[software]?.label || software;
}

function getServerIcon(server) {
    return SERVER_SOFTWARE[server?.software]?.icon || (server?.mode === 'plugins' ? 'bi-plug-fill' : 'bi-puzzle-fill');
}

function getServerContentLabel(server) {
    return server?.mode === 'plugins' ? 'Plugins' : 'Mods';
}

function getServerSoftwareOptions(mode) {
    return (SERVER_SOFTWARE_BY_MODE[mode] || SERVER_SOFTWARE_BY_MODE.plugins).map((value, index) => ({
        value,
        label: SERVER_SOFTWARE[value].label + (index === 0 ? ' (recommended)' : ''),
    }));
}

function renderServerSoftwareOptions(mode, selected = '') {
    return getServerSoftwareOptions(mode)
        .map(opt => `<option value="${escapeAttr(opt.value)}" ${opt.value === selected ? 'selected' : ''}>${escapeHtml(opt.label)}</option>`)
        .join('');
}

function updateServerSoftwareFieldByMode() {
    const modeSelect = document.getElementById('new-server-mode');
    const softwareSelect = document.getElementById('new-server-software');
    if (!modeSelect || !softwareSelect) return;

    const mode = modeSelect.value || 'plugins';
    const options = getServerSoftwareOptions(mode);
    const current = softwareSelect.value;
    const fallback = options[0]?.value || 'paper';
    const next = options.some(o => o.value === current) ? current : fallback;
    softwareSelect.innerHTML = renderServerSoftwareOptions(mode, next);
    updateServerCreateSoftwareDetails();
}

function formatServerMemory(mb) {
    const value = Number(mb || 0);
    if (value >= 1024) return `${(value / 1024).toFixed(value >= 10240 ? 0 : 1)} GB`;
    return `${Math.round(value)} MB`;
}

function formatServerRate(kbps) {
    const value = Number(kbps || 0);
    if (value >= 1024) return `${(value / 1024).toFixed(1)} MB/s`;
    return `${value.toFixed(value < 10 ? 1 : 0)} KB/s`;
}

function formatServerUptime(seconds) {
    const total = Math.max(0, Math.floor(Number(seconds || 0)));
    const h = Math.floor(total / 3600);
    const m = Math.floor((total % 3600) / 60);
    const s = total % 60;
    if (h > 0) return `${h} h ${m} min`;
    if (m > 0) return `${m} min ${s} s`;
    return `${s} s`;
}

function clearServerHubIntervals() {
    if (serverHubPollInterval) {
        clearInterval(serverHubPollInterval);
        serverHubPollInterval = null;
    }
    if (serverDetailsPollInterval) {
        clearInterval(serverDetailsPollInterval);
        serverDetailsPollInterval = null;
    }
    if (serverConsolePollInterval) {
        clearInterval(serverConsolePollInterval);
        serverConsolePollInterval = null;
    }
    stopServerMetricAnimation();
}

function animateServerTabContent(shell) {
    if (!shell) return;
    shell.classList.remove('server-tab-enter');
    void shell.offsetWidth;
    shell.classList.add('server-tab-enter');
}

function updateServerAutoScrollButton() {
    const btn = document.getElementById('server-auto-scroll-btn');
    if (!btn) return;
    btn.innerHTML = `<i class="bi bi-arrow-down-square"></i> Auto-Scroll: ${serverConsoleAutoScroll ? 'On' : 'Off'}`;
    btn.style.color = serverConsoleAutoScroll ? 'var(--text-primary)' : 'var(--text-secondary)';
}

function toggleServerConsoleAutoScroll() {
    serverConsoleAutoScroll = !serverConsoleAutoScroll;
    updateServerAutoScrollButton();
    if (serverConsoleAutoScroll) {
        const el = document.getElementById('server-console-output');
        if (el) {
            el.scrollTop = el.scrollHeight;
        }
    }
}

function getServerCardTemplate(server) {
    const modeLabel = getServerModeLabel(server.mode);
    const safeId = escapeAttr(server.id);

    return `
        <div class="server-card server-card-rich" onclick="openServerProfileDetails('${safeId}')">
            <div class="server-card-top">
                <div class="server-card-icon"><i class="bi ${getServerIcon(server)}"></i></div>
                <div style="flex:1; min-width:0;">
                    <h3 class="server-card-title">${escapeHtml(server.name)}</h3>
                    <div class="server-card-subtitle">${escapeHtml(getServerSoftwareLabel(server.software))} • MC ${escapeHtml(server.minecraft_version)} • ${modeLabel}</div>
                </div>
            </div>

            <div class="server-runtime-badge offline" id="server-runtime-${safeId}"><i class="bi bi-circle-fill"></i> Status wird geladen...</div>
            <div class="server-load-row" id="server-load-${safeId}">&nbsp;</div>

            <div class="server-card-actions" onclick="event.stopPropagation()">
                <button class="btn" data-server-toggle-btn="${safeId}" onclick="toggleServerProfile('${safeId}')">
                    <i class="bi bi-play-fill"></i> Start
                </button>
            </div>
        </div>
    `;
}

async function loadServerHub() {
    const root = document.getElementById('servers-hub-content');
    if (!root) return;

    clearServerHubIntervals();
    currentServerProfile = null;
    root.innerHTML = '<div class="loading">Loading local servers...</div>';

    try {
        serverProfiles = await invoke('get_server_profiles');

        if (pendingServerDetailOpen && serverProfiles.some(s => s.id === pendingServerDetailOpen.id)) {
            const { id, tab } = pendingServerDetailOpen;
            pendingServerDetailOpen = null;
            openServerProfileDetails(id, tab);
            return;
        }
        pendingServerDetailOpen = null;
        recordNavigation({ type: 'page', page: 'servers-hub' });

        if (!serverProfiles || serverProfiles.length === 0) {
            root.innerHTML = `
                <div class="server-grid">
                    <div class="server-card server-card-create" onclick="openCreateServerProfileModal()">
                        <div style="text-align:center;">
                            <div style="font-size: 42px; margin-bottom: 8px;">+</div>
                            <div>Create your first local server</div>
                        </div>
                    </div>
                </div>
            `;
            return;
        }

        const cards = serverProfiles.map(server => getServerCardTemplate(server)).join('');

        root.innerHTML = `
            <div style="display:flex; justify-content:flex-end; margin-bottom:15px;">
                <button class="btn" onclick="openCreateServerProfileModal()"><i class="bi bi-plus-circle"></i> New Server</button>
            </div>
            <div class="server-grid">
                ${cards}
                <div class="server-card server-card-create" onclick="openCreateServerProfileModal()">
                    <div style="text-align:center;">
                        <div style="font-size: 36px; margin-bottom: 8px;">+</div>
                        <div>Create another server</div>
                    </div>
                </div>
            </div>
        `;

        serverProfiles.forEach(server => refreshServerRuntimeUI(server.id, false));
        serverHubPollInterval = setInterval(() => {
            if (currentPage !== 'servers-hub') return;
            serverProfiles.forEach(server => refreshServerRuntimeUI(server.id, false));
        }, 1600);
    } catch (error) {
        root.innerHTML = `<div style="color:#f44336;">Failed to load servers: ${escapeHtml(String(error))}</div>`;
    }
}

const serverSoftwareVersionsCache = new Map();

async function getServerSoftwareVersions(software) {
    if (serverSoftwareVersionsCache.has(software)) {
        return serverSoftwareVersionsCache.get(software);
    }
    const versions = await invoke('get_server_software_versions', { software });
    if (Array.isArray(versions) && versions.length > 0) {
        serverSoftwareVersionsCache.set(software, versions);
    }
    return versions || [];
}

async function openCreateServerProfileModal() {
    const existing = document.getElementById('create-server-profile-modal');
    if (existing) existing.remove();

    const modal = document.createElement('div');
    modal.id = 'create-server-profile-modal';
    modal.className = 'modal active';
    modal.innerHTML = `
        <div class="modal-content" style="max-width: 560px;">
            <div class="modal-header"><i class="bi bi-hdd-network"></i> Create Local Server</div>
            <div class="form-group">
                <label>Server Name</label>
                <input type="text" id="new-server-name" placeholder="My SMP Server">
            </div>
            <div class="form-group">
                <label>Mode</label>
                <select id="new-server-mode">
                    <option value="plugins">Plugins (Paper, Purpur, Folia, Spigot, Bukkit)</option>
                    <option value="modded">Modded (Fabric, Quilt, Forge, NeoForge)</option>
                </select>
            </div>
            <div class="form-group">
                <label>Software</label>
                <select id="new-server-software">${renderServerSoftwareOptions('plugins', 'paper')}</select>
                <div id="new-server-software-hint" class="server-create-hint"></div>
            </div>
            <div class="form-group">
                <label>Minecraft Version</label>
                <select id="new-server-mc-version"><option value="">Lade verfügbare Versionen...</option></select>
                <div id="new-server-version-status" class="server-create-hint"></div>
            </div>
            <div class="form-group" id="new-server-software-version-group">
                <label>Software Version / Build</label>
                <input type="text" id="new-server-software-version" placeholder="latest" value="latest">
            </div>
            <div class="form-group">
                <label>Port</label>
                <input type="number" id="new-server-port" min="1" max="65535" value="25565" placeholder="25565">
            </div>
            <div class="modal-actions">
                <button class="btn btn-secondary" onclick="closeCreateServerProfileModal()">Cancel</button>
                <button class="btn" id="new-server-submit" onclick="submitCreateServerProfile()">Create</button>
            </div>
        </div>
    `;

    document.body.appendChild(modal);
    document.getElementById('new-server-mode')?.addEventListener('change', updateServerSoftwareFieldByMode);
    document.getElementById('new-server-software')?.addEventListener('change', updateServerCreateSoftwareDetails);

    updateServerCreateSoftwareDetails();
}

async function updateServerCreateSoftwareDetails() {
    const software = document.getElementById('new-server-software')?.value || 'paper';
    const info = SERVER_SOFTWARE[software] || {};

    const hint = document.getElementById('new-server-software-hint');
    if (hint) hint.textContent = info.hint || '';

    const versionGroup = document.getElementById('new-server-software-version-group');
    const versionInput = document.getElementById('new-server-software-version');
    if (versionGroup && versionInput) {
        versionGroup.style.display = info.versionHint ? '' : 'none';
        versionInput.placeholder = info.versionHint || 'latest';
        if (!info.versionHint) versionInput.value = 'latest';
    }

    const select = document.getElementById('new-server-mc-version');
    const status = document.getElementById('new-server-version-status');
    const submit = document.getElementById('new-server-submit');
    if (!select) return;
    const previous = select.value;

    select.disabled = true;
    if (submit) submit.disabled = true;
    select.innerHTML = '<option value="">Lade verfügbare Versionen...</option>';
    if (status) status.textContent = '';

    let versions = [];
    let loadError = null;
    try {
        versions = await getServerSoftwareVersions(software);
    } catch (error) {
        loadError = error;
    }

    // Das Modal könnte inzwischen geschlossen oder die Software geändert worden sein
    if (document.getElementById('new-server-software')?.value !== software) return;

    if (loadError || versions.length === 0) {
        select.innerHTML = '<option value="">Keine Versionen verfügbar</option>';
        if (status) {
            status.innerHTML = `<span style="color:#f44336;"><i class="bi bi-exclamation-triangle"></i> Versionen für ${escapeHtml(info.label || software)} konnten nicht geladen werden${loadError ? ': ' + escapeHtml(String(loadError)) : ''}</span>
                <a href="#" onclick="serverSoftwareVersionsCache.delete('${escapeAttr(software)}'); updateServerCreateSoftwareDetails(); return false;" style="color: var(--gold); margin-left:6px;">Erneut versuchen</a>`;
        }
        return;
    }

    select.innerHTML = versions
        .map(v => `<option value="${escapeAttr(v)}">${escapeHtml(v)}</option>`)
        .join('');
    if (previous && versions.includes(previous)) {
        select.value = previous;
    }
    select.disabled = false;
    if (submit) submit.disabled = false;
    if (status) status.textContent = `${versions.length} Versionen für ${info.label || software} verfügbar`;
}

function closeCreateServerProfileModal() {
    const modal = document.getElementById('create-server-profile-modal');
    if (modal) modal.remove();
}

async function submitCreateServerProfile() {
    const name = document.getElementById('new-server-name')?.value.trim() || '';
    const mode = document.getElementById('new-server-mode')?.value || 'plugins';
    const software = document.getElementById('new-server-software')?.value || 'paper';
    const minecraftVersion = document.getElementById('new-server-mc-version')?.value || '';
    const softwareVersion = document.getElementById('new-server-software-version')?.value.trim() || 'latest';
    const rawPort = parseInt(document.getElementById('new-server-port')?.value || '25565', 10);
    const port = Number.isFinite(rawPort) && rawPort > 0 && rawPort <= 65535 ? rawPort : 25565;

    if (!name) {
        showToast('Please enter a server name', 'error', 2500);
        return;
    }
    if (!minecraftVersion) {
        showToast('Please select a Minecraft version', 'error', 2500);
        return;
    }

    try {
        await invoke('create_server_profile', {
            name,
            mode,
            software,
            minecraftVersion,
            softwareVersion,
            port,
        });
        closeCreateServerProfileModal();
        await loadServerHub();
        showToast('Server profile created', 'success', 2000);
    } catch (error) {
        showToast('Failed to create server: ' + error, 'error', 4500);
    }
}

function openServerProfileDetails(serverId, initialTab = 'console') {
    const root = document.getElementById('servers-hub-content');
    if (!root) return;
    const server = serverProfiles.find(s => s.id === serverId);
    if (!server) return;

    recordNavigation({ type: 'server', serverId, tab: initialTab });
    clearServerHubIntervals();
    currentServerProfile = server;
    currentServerFilesPath = '';
    currentServerEditedFile = null;
    serverMetricLastRuntime = null;
    if (!serverMetricHistory.has(server.id)) {
        serverMetricHistory.set(server.id, []);
    }

    const safeId = escapeAttr(server.id);
    root.innerHTML = `
        <div class="server-detail-header">
            <div class="server-detail-top">
                <div class="server-detail-icon"><i class="bi ${getServerIcon(server)}"></i></div>
                <div style="flex:1; min-width:0;">
                    <h2 class="server-detail-name">${escapeHtml(server.name)}</h2>
                    <div class="server-detail-meta">${escapeHtml(getServerSoftwareLabel(server.software))} • ${getServerModeLabel(server.mode)} • Minecraft ${escapeHtml(server.minecraft_version)}</div>
                    <div id="server-detail-runtime" class="server-runtime-badge offline" style="margin-top:8px; width:fit-content;"><i class="bi bi-circle-fill"></i> Status wird geladen...</div>
                </div>
                <div style="display:flex; flex-direction:column; gap:8px; align-items:flex-end;">
                    <button class="server-address-badge" id="server-detail-address" title="Adresse kopieren" onclick="copyServerAddress('${safeId}')">
                        <i class="bi bi-clipboard"></i> 127.0.0.1:${server.port || 25565}
                    </button>
                    <div style="display:flex; gap:8px; flex-wrap:wrap; justify-content:flex-end;">
                        <button class="btn" id="server-detail-toggle-btn" data-server-toggle-btn="${safeId}" onclick="toggleServerProfile('${safeId}')">
                            <i class="bi bi-play-fill"></i> Start
                        </button>
                        <button class="btn btn-secondary" onclick="restartServerProfile('${safeId}')"><i class="bi bi-arrow-clockwise"></i> Restart</button>
                        <button class="btn btn-secondary" onclick="killServerProfile('${safeId}')"><i class="bi bi-lightning-charge"></i> Kill</button>
                    </div>
                </div>
            </div>
        </div>

        <div class="server-tab-shell">
            <div class="server-tab-inner">
                <button class="btn btn-secondary server-detail-tab" data-server-tab="console" onclick="switchServerDetailTab('console','${safeId}')"><i class="bi bi-terminal"></i> Console</button>
                <button class="btn btn-secondary server-detail-tab" data-server-tab="content" onclick="switchServerDetailTab('content','${safeId}')"><i class="bi ${server.mode === 'plugins' ? 'bi-plug' : 'bi-puzzle'}"></i> Content</button>
                <button class="btn btn-secondary server-detail-tab" data-server-tab="backups" onclick="switchServerDetailTab('backups','${safeId}')"><i class="bi bi-archive"></i> Backups</button>
                <button class="btn btn-secondary server-detail-tab" data-server-tab="settings" onclick="switchServerDetailTab('settings','${safeId}')"><i class="bi bi-sliders"></i> Settings</button>
                <button class="btn btn-secondary server-detail-tab" data-server-tab="files" onclick="switchServerDetailTab('files','${safeId}')"><i class="bi bi-folder2-open"></i> Files</button>
            </div>
        </div>
        <div id="server-detail-content" style="max-height: calc(100vh - 290px); overflow-y: auto; overflow-x: hidden; padding-right: 4px;"></div>
    `;

    switchServerDetailTab(initialTab, server.id);
    refreshServerRuntimeUI(server.id, false);
    serverDetailsPollInterval = setInterval(() => {
        if (currentPage !== 'servers-hub' || !currentServerProfile || currentServerProfile.id !== server.id) return;
        refreshServerRuntimeUI(server.id, false);
    }, 1000);
}

async function copyServerAddress(serverId) {
    const server = serverProfiles.find(s => s.id === serverId);
    const address = `127.0.0.1:${server?.port || 25565}`;
    try {
        await navigator.clipboard.writeText(address);
        showToast(`${address} kopiert`, 'success', 1800);
    } catch (_e) {
        showToast(address, 'info', 2500);
    }
}

function switchServerDetailTab(tab, serverId) {
    patchCurrentNavLocation(e => e.type === 'server' && e.serverId === serverId, { tab });
    document.querySelectorAll('.server-detail-tab').forEach(btn => {
        btn.classList.toggle('active', btn.dataset.serverTab === tab);
    });

    const shell = document.getElementById('server-detail-content');
    if (!shell) return;
    if (serverConsolePollInterval) {
        clearInterval(serverConsolePollInterval);
        serverConsolePollInterval = null;
    }
    stopServerMetricAnimation();

    const server = serverProfiles.find(s => s.id === serverId) || currentServerProfile;
    const safeId = escapeAttr(serverId);

    if (tab === 'content') {
        shell.innerHTML = renderServerContentTab(server);
        animateServerTabContent(shell);
        loadServerInstalledContent(serverId);
        return;
    }

    if (tab === 'backups') {
        shell.innerHTML = renderServerBackupsTab(server);
        animateServerTabContent(shell);
        loadServerBackups(serverId);
        return;
    }

    if (tab === 'console') {
        shell.innerHTML = `
            <div class="server-metrics-grid">
                ${renderServerMetricCard('cpu', 'bi-cpu', 'CPU')}
                ${renderServerMetricCard('ram', 'bi-memory', 'RAM')}
                ${renderServerMetricCard('net', 'bi-hdd-network', 'Netzwerk (System)', `
                    <div class="server-metric-legend">
                        <span><span class="server-metric-key" data-series="rx"></span>Download</span>
                        <span><span class="server-metric-key" data-series="tx"></span>Upload</span>
                    </div>
                `)}
            </div>

            <div class="server-console-panel">
                <div class="server-console-toolbar">
                    <div style="display:flex; gap:8px; flex-wrap:wrap;">
                        <button class="btn btn-secondary" onclick="loadServerConsole('${safeId}')"><i class="bi bi-arrow-clockwise"></i> Refresh</button>
                        <button class="btn btn-secondary" onclick="clearServerConsoleOutput()"><i class="bi bi-eraser"></i> Clear View</button>
                        <button class="btn btn-secondary" id="server-auto-scroll-btn" onclick="toggleServerConsoleAutoScroll()"></button>
                    </div>
                    <div style="color:var(--text-secondary); font-size:12px; display:flex; align-items:center; gap:6px;"><i class="bi bi-broadcast"></i> Live</div>
                </div>
                <pre id="server-console-output" class="profile-scroll-box server-console-output"></pre>
                <div style="display:flex; gap:8px; margin-top:10px;">
                    <input id="server-console-command" type="text" placeholder="Befehl eingeben, z.B. say Hallo" style="flex:1;">
                    <button class="btn" onclick="sendServerConsoleCommand('${safeId}')"><i class="bi bi-send"></i> Send</button>
                </div>
            </div>
        `;
        const input = document.getElementById('server-console-command');
        if (input) {
            input.addEventListener('keydown', (event) => {
                if (event.key === 'Enter') {
                    sendServerConsoleCommand(serverId);
                }
            });
        }

        ['cpu', 'ram', 'net'].forEach(key => bindServerMetricHover(key));
        animateServerTabContent(shell);
        updateServerAutoScrollButton();
        loadServerConsole(serverId);
        updateServerMetricCards(serverId, serverMetricLastRuntime);
        startServerMetricAnimation(serverId);
        serverConsolePollInterval = setInterval(() => {
            if (!currentServerProfile || currentServerProfile.id !== serverId) return;
            loadServerConsole(serverId);
        }, 1000);
        return;
    }

    if (tab === 'settings') {
        shell.innerHTML = `<div id="server-settings-content" style="background:var(--bg-medium); border-radius:10px; padding:12px;">Loading settings...</div>`;
        animateServerTabContent(shell);
        loadServerSettings(serverId);
        return;
    }

    shell.innerHTML = `
        <div style="display:flex; justify-content:space-between; gap:10px; margin-bottom:10px; flex-wrap:wrap;">
            <div style="display:flex; gap:8px; flex-wrap:wrap;">
                <button class="btn btn-secondary" onclick="navigateServerFilesUp('${safeId}')"><i class="bi bi-arrow-up"></i> Up</button>
                <button class="btn btn-secondary" onclick="loadServerFiles('${safeId}', currentServerFilesPath)"><i class="bi bi-arrow-clockwise"></i> Refresh</button>
                <button class="btn btn-secondary" onclick="createServerFolderPrompt('${safeId}')"><i class="bi bi-folder-plus"></i> New Folder</button>
                <button class="btn btn-secondary" onclick="triggerServerFileUpload()"><i class="bi bi-upload"></i> Upload</button>
                <button class="btn btn-secondary" onclick="openServerFolder('${safeId}', currentServerFilesPath || null)"><i class="bi bi-box-arrow-up-right"></i> Open Extern</button>
                <input id="server-file-upload" type="file" style="display:none;" onchange="uploadServerFileFromInput('${safeId}', this)">
            </div>
            <div id="server-files-path" style="font-size:12px; color:var(--text-secondary);">/</div>
        </div>
        <div id="server-files-list" class="profile-scroll-box" style="background:var(--bg-medium); border-radius:10px; padding:10px; min-height:240px;"></div>
        <div id="server-file-editor-shell" style="display:none; margin-top:12px; background:var(--bg-medium); border-radius:10px; padding:12px;">
            <div style="display:flex; justify-content:space-between; align-items:center; gap:10px; margin-bottom:8px;">
                <strong id="server-file-editor-name"></strong>
                <div style="display:flex; gap:8px;">
                    <button class="btn btn-secondary" onclick="closeServerFileEditor()">Close</button>
                    <button class="btn" onclick="saveServerFileEditor('${safeId}')">Save</button>
                </div>
            </div>
            <textarea id="server-file-editor" style="width:100%; min-height:260px; font-family:Consolas, monospace;"></textarea>
        </div>
    `;
    animateServerTabContent(shell);
    loadServerFiles(serverId, currentServerFilesPath || '');
}

function getServerRuntimeStatus(runtime) {
    if (runtime?.running) {
        return { cls: 'online', html: `<i class="bi bi-circle-fill"></i> Online · ${formatServerUptime(runtime.uptime_seconds)}` };
    }
    if (runtime?.installing) {
        return { cls: 'installing', html: '<i class="bi bi-hourglass-split"></i> Wird vorbereitet...' };
    }
    return { cls: 'offline', html: '<i class="bi bi-circle"></i> Offline' };
}

async function refreshServerRuntimeUI(serverId, showErrorToast = false) {
    try {
        const runtime = await invoke('get_server_runtime', { serverId });
        const running = !!runtime?.running;
        const installing = !!runtime?.installing;
        recordServerMetrics(serverId, runtime);

        const status = getServerRuntimeStatus(runtime);
        const loadText = running
            ? `CPU ${(runtime.cpu_percent || 0).toFixed(1)} % • RAM ${formatServerMemory(runtime.memory_mb)} • PID ${runtime.pid || '-'}`
            : installing
                ? 'Installation läuft – siehe Konsole'
                : 'Gestoppt';

        [document.getElementById(`server-runtime-${serverId}`),
            currentServerProfile?.id === serverId ? document.getElementById('server-detail-runtime') : null]
            .forEach(badge => {
                if (!badge) return;
                badge.innerHTML = status.html;
                badge.classList.remove('online', 'offline', 'installing');
                badge.classList.add(status.cls);
            });

        const loadRow = document.getElementById(`server-load-${serverId}`);
        if (loadRow) {
            loadRow.textContent = loadText;
        }

        if (currentServerProfile && currentServerProfile.id === serverId) {
            serverMetricLastRuntime = runtime;
            updateServerMetricCards(serverId, runtime);
        }

        document.querySelectorAll(`[data-server-toggle-btn="${serverId}"]`).forEach(btn => {
            btn.disabled = installing;
            if (running) {
                btn.innerHTML = '<i class="bi bi-stop-fill"></i> Stop';
                btn.style.background = '#c0392b';
                btn.style.color = '#fff';
            } else if (installing) {
                btn.innerHTML = '<i class="bi bi-hourglass-split"></i> Startet...';
                btn.style.background = '';
                btn.style.color = '';
            } else {
                btn.innerHTML = '<i class="bi bi-play-fill"></i> Start';
                btn.style.background = '';
                btn.style.color = '';
            }
        });
    } catch (error) {
        if (showErrorToast) {
            showToast('Server status konnte nicht geladen werden: ' + error, 'error', 3500);
        }
    }
}

// ==================== SERVER METRICS (CPU / RAM / NETZWERK) ====================

function renderServerMetricCard(key, icon, label, extra = '') {
    return `
        <div class="server-metric-card" data-metric="${key}">
            <div class="server-metric-head">
                <div style="min-width:0;">
                    <div class="server-metric-label"><i class="bi ${icon}"></i> ${label}</div>
                    <div class="server-metric-value" id="server-metric-${key}-value">--</div>
                </div>
                <div class="server-metric-side">
                    <div class="server-metric-sub" id="server-metric-${key}-sub">Server offline</div>
                    ${extra}
                </div>
            </div>
            <div class="server-metric-chart">
                <canvas id="server-metric-${key}-chart" aria-label="${label} Verlauf der letzten 2 Minuten" role="img"></canvas>
                <div class="server-metric-tooltip" id="server-metric-${key}-tooltip"></div>
            </div>
        </div>
    `;
}

function recordServerMetrics(serverId, runtime) {
    const history = serverMetricHistory.get(serverId) || [];
    const now = Date.now();

    if (runtime?.running) {
        history.push({
            ts: now,
            cpu: Number(runtime.cpu_percent || 0),
            ram: Number(runtime.memory_mb || 0),
            rx: Number(runtime.network_rx_kbps || 0),
            tx: Number(runtime.network_tx_kbps || 0),
        });
    } else if (history.length > 0 && !history[history.length - 1].gap) {
        // Lücke markieren, damit Start/Stop nicht mit einer Linie verbunden wird
        history.push({ ts: now, gap: true });
    }

    const cutoff = now - SERVER_METRIC_WINDOW_MS - SERVER_METRIC_RENDER_DELAY_MS - 5000;
    while (history.length > 0 && history[0].ts < cutoff) history.shift();
    serverMetricHistory.set(serverId, history);
}

function updateServerMetricCards(serverId, runtime) {
    if (!currentServerProfile || currentServerProfile.id !== serverId) return;
    const set = (id, text) => {
        const el = document.getElementById(id);
        if (el) el.textContent = text;
    };
    const allocated = currentServerProfile.memory_mb || 4096;

    if (!runtime?.running) {
        const offlineText = runtime?.installing ? 'Wird vorbereitet...' : 'Server offline';
        set('server-metric-cpu-value', '--');
        set('server-metric-cpu-sub', offlineText);
        set('server-metric-ram-value', '--');
        set('server-metric-ram-sub', `${formatServerMemory(allocated)} zugewiesen`);
        set('server-metric-net-value', '--');
        set('server-metric-net-sub', offlineText);
        return;
    }

    set('server-metric-cpu-value', `${Number(runtime.cpu_percent || 0).toFixed(1)} %`);
    set('server-metric-cpu-sub', `${runtime.cpu_cores || '?'} Kerne gesamt`);
    set('server-metric-ram-value', formatServerMemory(runtime.memory_mb));
    set('server-metric-ram-sub', `von ${formatServerMemory(allocated)} zugewiesen`);
    set('server-metric-net-value', `↓ ${formatServerRate(runtime.network_rx_kbps)}`);
    set('server-metric-net-sub', `↑ ${formatServerRate(runtime.network_tx_kbps)}`);
}

function niceServerChartMax(value) {
    if (!(value > 0)) return 1;
    const exponent = Math.floor(Math.log10(value));
    const base = Math.pow(10, exponent);
    const step = [1, 2, 2.5, 5, 10].find(s => s * base >= value) || 10;
    return step * base;
}

function hexToRgba(color, alpha) {
    const hex = String(color || '').trim().replace('#', '');
    const full = hex.length === 3 ? hex.split('').map(c => c + c).join('') : hex;
    if (!/^[0-9a-fA-F]{6}$/.test(full)) return `rgba(255, 215, 0, ${alpha})`;
    const n = parseInt(full, 16);
    return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
}

function isLightSurface(color) {
    const hex = String(color || '').trim().replace('#', '');
    const full = hex.length === 3 ? hex.split('').map(c => c + c).join('') : hex;
    if (!/^[0-9a-fA-F]{6}$/.test(full)) return false;
    const n = parseInt(full, 16);
    const luminance = (0.2126 * ((n >> 16) & 255) + 0.7152 * ((n >> 8) & 255) + 0.0722 * (n & 255)) / 255;
    return luminance > 0.6;
}

function getServerChartTheme(element) {
    const styles = getComputedStyle(element);
    const surface = styles.getPropertyValue('--bg-dark').trim() || '#1a1a1a';
    const light = isLightSurface(surface);
    return {
        accent: styles.getPropertyValue('--gold').trim() || '#FFD700',
        surface,
        text: styles.getPropertyValue('--text-secondary').trim() || '#b0b0b0',
        grid: light ? 'rgba(0, 0, 0, 0.09)' : 'rgba(255, 255, 255, 0.08)',
        crosshair: light ? 'rgba(0, 0, 0, 0.35)' : 'rgba(255, 255, 255, 0.35)',
        // Validierte Kategorie-Farben (blau/orange) je Theme-Oberfläche
        download: light ? '#2a78d6' : '#3987e5',
        upload: light ? '#eb6834' : '#d95926',
    };
}

function valueAtServerChartTime(points, key, ts) {
    // Linear interpolieren, damit die Linie exakt an der rechten Kante endet
    for (let i = points.length - 1; i > 0; i--) {
        const b = points[i];
        const a = points[i - 1];
        if (a.ts <= ts && b.ts >= ts) {
            if (a.gap || b.gap) return null;
            const t = (ts - a.ts) / Math.max(1, b.ts - a.ts);
            return { ts, value: a[key] + (b[key] - a[key]) * t };
        }
    }
    const last = points[points.length - 1];
    if (last && !last.gap && last.ts <= ts) return { ts: last.ts, value: last[key] };
    return null;
}

function drawServerMetricChart(canvas, config) {
    const rect = canvas.getBoundingClientRect();
    if (rect.width < 10 || rect.height < 10) return null;

    const dpr = window.devicePixelRatio || 1;
    const pixelWidth = Math.round(rect.width * dpr);
    const pixelHeight = Math.round(rect.height * dpr);
    if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
        canvas.width = pixelWidth;
        canvas.height = pixelHeight;
    }

    const ctx = canvas.getContext('2d');
    if (!ctx) return null;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, rect.width, rect.height);

    const theme = config.theme;
    const pad = { left: 52, right: 10, top: 8, bottom: 20 };
    const plotWidth = Math.max(1, rect.width - pad.left - pad.right);
    const plotHeight = Math.max(1, rect.height - pad.top - pad.bottom);
    const end = config.now;
    const start = end - SERVER_METRIC_WINDOW_MS;
    const toX = ts => pad.left + ((ts - start) / SERVER_METRIC_WINDOW_MS) * plotWidth;
    const toY = value => pad.top + plotHeight - (Math.max(0, Math.min(value, config.max)) / config.max) * plotHeight;

    ctx.font = '11px "Segoe UI", system-ui, sans-serif';
    ctx.lineWidth = 1;

    // Gridlines + Y-Achse (0 / 50 % / 100 % der Skala)
    ctx.textAlign = 'right';
    ctx.textBaseline = 'middle';
    [0, 0.5, 1].forEach(fraction => {
        const y = Math.round(pad.top + plotHeight - fraction * plotHeight) + 0.5;
        ctx.strokeStyle = theme.grid;
        ctx.beginPath();
        ctx.moveTo(pad.left, y);
        ctx.lineTo(pad.left + plotWidth, y);
        ctx.stroke();
        ctx.fillStyle = theme.text;
        ctx.fillText(config.formatTick(config.max * fraction), pad.left - 8, y);
    });

    // X-Achse
    ctx.textBaseline = 'top';
    const labelY = pad.top + plotHeight + 6;
    ctx.textAlign = 'left';
    ctx.fillText('-2 min', pad.left, labelY);
    ctx.textAlign = 'center';
    ctx.fillText('-1 min', pad.left + plotWidth / 2, labelY);
    ctx.textAlign = 'right';
    ctx.fillText('jetzt', pad.left + plotWidth, labelY);

    const points = config.points;
    const hasData = points.some(p => !p.gap && p.ts >= start - 5000);
    if (!hasData) {
        ctx.textAlign = 'center';
        ctx.textBaseline = 'middle';
        ctx.fillStyle = theme.text;
        ctx.fillText(config.emptyText, pad.left + plotWidth / 2, pad.top + plotHeight / 2);
        return null;
    }

    ctx.save();
    ctx.beginPath();
    ctx.rect(pad.left, pad.top - 6, plotWidth, plotHeight + 12);
    ctx.clip();

    // In zusammenhängende Segmente aufteilen (Lücken = Server war offline)
    const segments = [];
    let current = [];
    points.forEach(point => {
        if (point.gap) {
            if (current.length) segments.push(current);
            current = [];
        } else {
            current.push(point);
        }
    });
    if (current.length) segments.push(current);

    const baseline = pad.top + plotHeight;
    config.series.forEach(series => {
        segments.forEach(segment => {
            if (config.fill && segment.length > 1) {
                ctx.beginPath();
                ctx.moveTo(toX(segment[0].ts), baseline);
                segment.forEach(p => ctx.lineTo(toX(p.ts), toY(p[series.key])));
                ctx.lineTo(toX(segment[segment.length - 1].ts), baseline);
                ctx.closePath();
                ctx.fillStyle = hexToRgba(series.color, 0.1);
                ctx.fill();
            }

            ctx.beginPath();
            segment.forEach((p, index) => {
                const x = toX(p.ts);
                const y = toY(p[series.key]);
                if (index === 0) ctx.moveTo(x, y);
                else ctx.lineTo(x, y);
            });
            ctx.strokeStyle = series.color;
            ctx.lineWidth = 2;
            ctx.lineJoin = 'round';
            ctx.lineCap = 'round';
            ctx.stroke();
        });
    });
    ctx.restore();

    const drawDot = (x, y, color) => {
        ctx.beginPath();
        ctx.arc(x, y, 4, 0, Math.PI * 2);
        ctx.fillStyle = color;
        ctx.fill();
        ctx.lineWidth = 2;
        ctx.strokeStyle = theme.surface;
        ctx.stroke();
    };

    // Hover: Crosshair rastet auf den nächsten Messpunkt ein
    if (config.hoverX != null) {
        const visible = points.filter(p => !p.gap && p.ts >= start && p.ts <= end);
        if (visible.length > 0) {
            const nearest = visible.reduce((best, p) =>
                Math.abs(toX(p.ts) - config.hoverX) < Math.abs(toX(best.ts) - config.hoverX) ? p : best);
            const x = Math.round(toX(nearest.ts)) + 0.5;
            ctx.strokeStyle = theme.crosshair;
            ctx.lineWidth = 1;
            ctx.beginPath();
            ctx.moveTo(x, pad.top);
            ctx.lineTo(x, baseline);
            ctx.stroke();
            config.series.forEach(series => drawDot(x, toY(nearest[series.key]), series.color));
            return { point: nearest, x };
        }
        return null;
    }

    // Endpunkt an der rechten Kante
    config.series.forEach(series => {
        const edge = valueAtServerChartTime(points, series.key, end);
        if (edge) drawDot(toX(edge.ts), toY(edge.value), series.color);
    });
    return null;
}

function updateServerMetricTooltip(key, hit, series, formatValue, now) {
    const tooltip = document.getElementById(`server-metric-${key}-tooltip`);
    if (!tooltip) return;
    if (!hit) {
        tooltip.classList.remove('visible');
        return;
    }

    tooltip.replaceChildren();
    series.forEach(s => {
        const row = document.createElement('div');
        row.className = 'server-metric-tooltip-row';
        const keyEl = document.createElement('span');
        keyEl.className = 'server-metric-key';
        keyEl.style.background = s.color;
        const value = document.createElement('strong');
        value.textContent = formatValue(hit.point[s.key]);
        const label = document.createElement('span');
        label.textContent = s.label;
        row.append(keyEl, value, label);
        tooltip.appendChild(row);
    });
    const age = document.createElement('div');
    age.className = 'server-metric-tooltip-time';
    const seconds = Math.max(0, Math.round((now + SERVER_METRIC_RENDER_DELAY_MS - hit.point.ts) / 1000));
    age.textContent = seconds <= 1 ? 'gerade eben' : `vor ${seconds} s`;
    tooltip.appendChild(age);

    const chart = tooltip.parentElement;
    const maxLeft = (chart?.clientWidth || 0) - tooltip.offsetWidth - 4;
    const left = hit.x + 12 > maxLeft ? hit.x - tooltip.offsetWidth - 12 : hit.x + 12;
    tooltip.style.left = `${Math.max(4, left)}px`;
    tooltip.classList.add('visible');
}

function renderServerMetricCharts(serverId) {
    const cpuCanvas = document.getElementById('server-metric-cpu-chart');
    if (!cpuCanvas || !currentServerProfile || currentServerProfile.id !== serverId) return false;

    const theme = getServerChartTheme(cpuCanvas);
    const points = serverMetricHistory.get(serverId) || [];
    const now = Date.now() - SERVER_METRIC_RENDER_DELAY_MS;
    const valid = points.filter(p => !p.gap);
    const emptyText = serverMetricLastRuntime?.installing ? 'Server wird vorbereitet...' : 'Keine Daten – Server ist offline';
    const allocated = currentServerProfile.memory_mb || 4096;

    document.querySelectorAll('.server-metric-key[data-series="rx"]').forEach(el => { el.style.background = theme.download; });
    document.querySelectorAll('.server-metric-key[data-series="tx"]').forEach(el => { el.style.background = theme.upload; });

    const charts = [
        {
            key: 'cpu',
            series: [{ key: 'cpu', label: 'CPU', color: theme.accent }],
            max: 100,
            fill: true,
            formatTick: v => `${Math.round(v)} %`,
            formatValue: v => `${Number(v).toFixed(1)} %`,
        },
        {
            key: 'ram',
            series: [{ key: 'ram', label: 'RAM', color: theme.accent }],
            max: Math.ceil(Math.max(allocated, ...valid.map(p => p.ram * 1.05)) / 512) * 512,
            fill: true,
            formatTick: v => formatServerMemory(v),
            formatValue: v => formatServerMemory(v),
        },
        {
            key: 'net',
            series: [
                { key: 'rx', label: 'Download', color: theme.download },
                { key: 'tx', label: 'Upload', color: theme.upload },
            ],
            max: niceServerChartMax(Math.max(16, ...valid.map(p => Math.max(p.rx, p.tx) * 1.15))),
            fill: false,
            formatTick: v => formatServerRate(v),
            formatValue: v => formatServerRate(v),
        },
    ];

    charts.forEach(chart => {
        const canvas = document.getElementById(`server-metric-${chart.key}-chart`);
        if (!canvas) return;
        const hit = drawServerMetricChart(canvas, {
            theme,
            points,
            now,
            max: chart.max,
            fill: chart.fill,
            series: chart.series,
            formatTick: chart.formatTick,
            emptyText,
            hoverX: serverMetricHover[chart.key],
        });
        updateServerMetricTooltip(chart.key, hit, chart.series, chart.formatValue, now);
    });
    return true;
}

function bindServerMetricHover(key) {
    const canvas = document.getElementById(`server-metric-${key}-chart`);
    if (!canvas) return;
    canvas.addEventListener('pointermove', event => {
        const rect = canvas.getBoundingClientRect();
        serverMetricHover[key] = event.clientX - rect.left;
    });
    canvas.addEventListener('pointerleave', () => {
        serverMetricHover[key] = null;
    });
}

function startServerMetricAnimation(serverId) {
    stopServerMetricAnimation();
    let lastFrame = 0;
    const frame = (timestamp) => {
        // ~30 FPS reichen für eine weich scrollende Zeitachse
        if (timestamp - lastFrame >= 33) {
            lastFrame = timestamp;
            if (!renderServerMetricCharts(serverId)) {
                serverMetricAnimationFrame = null;
                return;
            }
        }
        serverMetricAnimationFrame = requestAnimationFrame(frame);
    };
    serverMetricAnimationFrame = requestAnimationFrame(frame);
}

function stopServerMetricAnimation() {
    if (serverMetricAnimationFrame) {
        cancelAnimationFrame(serverMetricAnimationFrame);
        serverMetricAnimationFrame = null;
    }
    serverMetricHover.cpu = null;
    serverMetricHover.ram = null;
    serverMetricHover.net = null;
}

// ==================== SERVER ACTIONS ====================

async function toggleServerProfile(serverId) {
    try {
        const runtime = await invoke('get_server_runtime', { serverId });
        if (runtime?.running) {
            showToast('Server wird gestoppt...', 'info', 1800);
            const stopPromise = invoke('stop_server_profile', { serverId });
            setTimeout(() => refreshServerRuntimeUI(serverId, false), 400);
            await stopPromise;
        } else if (!runtime?.installing) {
            showToast('Server wird gestartet (Software wird bei Bedarf automatisch heruntergeladen)', 'success', 2600);
            const startPromise = invoke('start_server_profile', { serverId });
            setTimeout(() => refreshServerRuntimeUI(serverId, false), 300);
            await startPromise;
        }
        refreshServerRuntimeUI(serverId, false);
    } catch (error) {
        refreshServerRuntimeUI(serverId, false);
        showToast('Server Aktion fehlgeschlagen: ' + error, 'error', 5200);
    }
}

async function restartServerProfile(serverId) {
    try {
        showToast('Server wird neugestartet...', 'info', 2200);
        const restartPromise = invoke('restart_server_profile', { serverId });
        setTimeout(() => refreshServerRuntimeUI(serverId, false), 500);
        await restartPromise;
        refreshServerRuntimeUI(serverId, false);
    } catch (error) {
        showToast('Neustart fehlgeschlagen: ' + error, 'error', 4200);
    }
}

async function killServerProfile(serverId) {
    const confirmed = await showConfirmModal({
        title: 'Server killen?',
        message: 'Möchtest du den Server-Prozess wirklich sofort beenden? Nicht gespeicherte Weltdaten gehen dabei verloren.',
        confirmText: 'Kill',
        icon: 'bi-lightning-charge',
    });
    if (!confirmed) return;
    try {
        await invoke('kill_server_profile', { serverId });
        showToast('Server-Prozess wurde gekillt', 'info', 2200);
        setTimeout(() => refreshServerRuntimeUI(serverId, false), 250);
    } catch (error) {
        showToast('Kill fehlgeschlagen: ' + error, 'error', 3500);
    }
}

function fileToBase64(file) {
    return new Promise((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => {
            const result = String(reader.result || '');
            const base64 = result.includes(',') ? result.split(',')[1] : result;
            resolve(base64);
        };
        reader.onerror = () => reject(reader.error);
        reader.readAsDataURL(file);
    });
}

// ==================== SERVER CONTENT (PLUGINS / MODS) ====================

function renderServerContentTab(server) {
    const label = getServerContentLabel(server);
    const safeId = escapeAttr(server.id);

    if (server.software === 'vanilla') {
        return `
            <div class="server-panel" style="text-align:center; padding:50px 20px; color:var(--text-secondary);">
                <div style="font-size:42px; margin-bottom:12px;"><i class="bi bi-slash-circle"></i></div>
                <p style="margin:0 0 6px; color:var(--text-primary); font-weight:600;">Vanilla-Server unterstützen keine ${label}</p>
                <p style="margin:0; font-size:13px;">Erstelle einen ${server.mode === 'plugins' ? 'Paper-, Purpur-, Folia- oder Spigot' : 'Fabric-, Quilt-, Forge- oder NeoForge'}-Server, um ${label} zu installieren.</p>
            </div>
        `;
    }

    return `
        <div class="server-panel">
            <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px; gap:10px; flex-wrap:wrap;">
                <h3 style="color: var(--gold); margin: 0;">Installierte ${label}</h3>
                <div style="display: flex; gap: 8px; align-items:center; flex-wrap:wrap;">
                    <button class="btn btn-secondary" onclick="checkServerContentUpdates('${safeId}')" style="padding: 8px 12px; font-size: 12px;">
                        <i class="bi bi-search"></i> Updates
                    </button>
                    <button class="btn btn-secondary" onclick="loadServerInstalledContent('${safeId}')" style="padding: 8px 12px; font-size: 12px;" title="Aktualisieren">
                        <i class="bi bi-arrow-clockwise"></i>
                    </button>
                    <button class="btn btn-secondary" onclick="triggerServerContentUpload()" style="padding: 8px 12px; font-size: 12px;" title=".jar importieren">
                        <i class="bi bi-upload"></i>
                    </button>
                    <button class="btn btn-secondary" onclick="openServerFolder('${safeId}', '${server.mode === 'plugins' ? 'plugins' : 'mods'}')" style="padding: 8px 12px; font-size: 12px;" title="Ordner öffnen">
                        <i class="bi bi-folder"></i>
                    </button>
                    <input id="server-content-upload" type="file" accept=".jar" style="display:none;" onchange="uploadServerContentFile('${safeId}', '${server.mode === 'plugins' ? 'plugins' : 'mods'}', this)">
                    <button class="server-add-content-btn" onclick="openServerContentBrowser('${safeId}')">
                        + Add ${label}
                    </button>
                </div>
            </div>

            <div id="server-content-updates" style="display:none;"></div>

            <div style="position: relative; margin-bottom: 12px;">
                <i class="bi bi-search" style="position: absolute; left: 10px; top: 50%; transform: translateY(-50%); color: var(--text-secondary); font-size: 13px; pointer-events: none;"></i>
                <input type="text" id="server-content-search" class="server-content-search"
                       placeholder="Installierte ${label} durchsuchen..."
                       oninput="filterServerInstalledContent(this.value)">
            </div>

            <div id="server-content-bulk-bar" class="server-bulk-bar" style="display:none;">
                <label style="display: flex; align-items: center; gap: 8px; cursor: pointer; color: var(--text-secondary);">
                    <input type="checkbox" id="server-content-select-all" onchange="toggleSelectAllServerContent()" style="width: 16px; height: 16px; cursor: pointer; accent-color: var(--gold);">
                    <span id="server-content-selected-count">0 ausgewählt</span>
                </label>
                <div style="flex: 1;"></div>
                <button class="btn btn-secondary" onclick="bulkToggleServerContent('${safeId}', true)" style="padding: 6px 12px; font-size: 11px;">
                    <i class="bi bi-check-circle"></i> Aktivieren
                </button>
                <button class="btn btn-secondary" onclick="bulkToggleServerContent('${safeId}', false)" style="padding: 6px 12px; font-size: 11px;">
                    <i class="bi bi-pause-circle"></i> Deaktivieren
                </button>
                <button class="btn btn-secondary" onclick="bulkDeleteServerContent('${safeId}')" style="padding: 6px 12px; font-size: 11px; color: #f44336;">
                    <i class="bi bi-trash"></i> Delete
                </button>
            </div>

            <div id="server-content-list" class="profile-scroll-box" style="display: grid; gap: 8px; align-content:start; height: 420px; max-height: 420px; overflow-y: auto; overflow-x: hidden; padding-right: 5px;">
                <div style="text-align: center; padding: 40px; color: var(--text-secondary);">
                    <div class="spinner" style="margin: 0 auto 15px;"></div>
                    <p>Lade ${label}...</p>
                </div>
            </div>
            <div style="margin-top:10px; font-size:12px; color:var(--text-secondary);">
                <i class="bi bi-info-circle"></i> Änderungen an ${label} werden erst nach einem Server-Neustart aktiv.
            </div>
        </div>
    `;
}

function triggerServerContentUpload() {
    const el = document.getElementById('server-content-upload');
    if (el) el.click();
}

async function uploadServerContentFile(serverId, contentType, input) {
    const file = input.files?.[0];
    if (!file) return;

    try {
        const base64 = await fileToBase64(file);
        await invoke('import_server_content_file', {
            serverId,
            contentType,
            fileName: file.name,
            fileBase64: base64,
        });
        showToast(`${file.name} hinzugefügt`, 'success', 2500);
        await loadServerInstalledContent(serverId);
    } catch (error) {
        showToast('Upload fehlgeschlagen: ' + error, 'error', 4000);
    } finally {
        input.value = '';
    }
}

async function loadServerInstalledContent(serverId) {
    const list = document.getElementById('server-content-list');
    if (!list) return;

    selectedServerContent.clear();
    updateServerContentBulkBar();

    try {
        const items = await invoke('get_server_installed_content', { serverId });
        serverInstalledContentCache = items || [];
        serverInstalledProjectIds = new Set(serverInstalledContentCache.map(i => i.project_id).filter(Boolean));
        serverInstalledProjectIdsServerId = serverId;

        const search = document.getElementById('server-content-search');
        if (search) search.value = '';

        const bulkBar = document.getElementById('server-content-bulk-bar');
        if (bulkBar) bulkBar.style.display = serverInstalledContentCache.length ? 'flex' : 'none';

        renderServerInstalledContent(serverInstalledContentCache, serverId);
    } catch (error) {
        list.innerHTML = `
            <div style="text-align: center; padding: 40px; color: #f44336;">
                <div style="font-size: 48px; margin-bottom: 15px;"><i class="bi bi-x-circle-fill"></i></div>
                <p>Fehler beim Laden</p>
                <p style="font-size: 12px; color: var(--text-secondary);">${escapeHtml(String(error))}</p>
            </div>
        `;
    }
}

function filterServerInstalledContent(query) {
    const q = String(query || '').trim().toLowerCase();
    const filtered = q
        ? serverInstalledContentCache.filter(item =>
            [item.name, item.filename, item.version].some(v => String(v || '').toLowerCase().includes(q)))
        : serverInstalledContentCache;
    if (currentServerProfile) {
        renderServerInstalledContent(filtered, currentServerProfile.id, q.length > 0);
    }
}

function renderServerInstalledContent(items, serverId, isFiltered = false) {
    const list = document.getElementById('server-content-list');
    if (!list) return;
    const server = serverProfiles.find(s => s.id === serverId) || currentServerProfile;
    const label = getServerContentLabel(server);
    const safeId = escapeAttr(serverId);

    if (!items.length) {
        list.innerHTML = isFiltered
            ? `<div style="text-align: center; padding: 40px; color: var(--text-secondary);">
                   <i class="bi bi-search" style="font-size: 32px; display: block; margin-bottom: 12px;"></i>
                   <p>Keine ${label} gefunden</p>
               </div>`
            : `<div style="text-align: center; padding: 60px 20px; color: var(--text-secondary);">
                   <div style="font-size: 48px; margin-bottom: 15px;"><i class="bi bi-box"></i></div>
                   <p>Noch keine ${label} installiert</p>
                   <p style="font-size: 14px; margin-top: 10px;">
                       Klicke auf <a href="#" onclick="openServerContentBrowser('${safeId}'); return false;" style="color: var(--gold);">+ Add ${label}</a>, um ${label} zu installieren
                   </p>
               </div>`;
        return;
    }

    const defaultIcon = server?.mode === 'plugins' ? 'bi-plug' : 'bi-box';
    list.innerHTML = items.map(item => {
        const file = escapeAttr(item.filename);
        const jsFile = escapeAttr(String(item.filename).replace(/\\/g, '\\\\').replace(/'/g, "\\'"));
        const clickable = !!item.project_id;
        const checked = selectedServerContent.has(item.filename) ? 'checked' : '';
        const icon = item.icon_url
            ? `<img alt="" src="${escapeAttr(item.icon_url)}" style="width: 100%; height: 100%; object-fit: cover;" data-fallback="icon">`
            : `<span style="font-size: 22px;"><i class="bi ${defaultIcon}"></i></span>`;

        return `
            <div class="installed-mod-card server-content-card ${item.disabled ? 'disabled' : ''}" data-filename="${file}"
                 ${clickable ? `onclick="if(!event.target.closest('input, button')) { showServerContentDetails('${safeId}', '${escapeAttr(item.project_id)}'); }"` : ''}
                 style="cursor: ${clickable ? 'pointer' : 'default'};">
                <input type="checkbox" class="server-content-checkbox" data-filename="${file}" ${checked}
                       onchange="toggleServerContentSelection('${jsFile}')"
                       onclick="event.stopPropagation();"
                       style="width: 18px; height: 18px; cursor: pointer; flex-shrink: 0; accent-color: var(--gold);">
                <div class="server-content-icon">${icon}</div>
                <div style="flex: 1; min-width: 0;">
                    <div style="display: flex; align-items: center; gap: 8px; flex-wrap: wrap;">
                        <h4 style="margin: 0; color: var(--text-primary); font-size: 14px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 260px;">${escapeHtml(item.name)}</h4>
                        ${item.disabled ? '<span class="server-content-tag danger">DEAKTIVIERT</span>' : ''}
                        ${item.has_update ? '<span class="server-content-tag accent">UPDATE</span>' : ''}
                    </div>
                    <p style="margin: 3px 0 0 0; color: var(--text-secondary); font-size: 11px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">
                        ${escapeHtml(item.filename)} • ${formatBytes(item.size || 0)}
                    </p>
                    ${item.version ? `<p style="margin: 2px 0 0 0; color: var(--gold); font-size: 10px;">v${escapeHtml(item.version)}</p>` : ''}
                </div>
                <div style="display: flex; gap: 6px; flex-shrink: 0;">
                    <button class="btn btn-secondary" onclick="event.stopPropagation(); toggleServerContentItem('${safeId}', '${jsFile}', ${item.disabled})"
                            style="padding: 5px 10px; font-size: 11px;" title="${item.disabled ? 'Aktivieren' : 'Deaktivieren'}">
                        ${item.disabled ? '<i class="bi bi-check"></i>' : '<i class="bi bi-pause-fill"></i>'}
                    </button>
                    <button class="btn btn-secondary" onclick="event.stopPropagation(); deleteServerContentItem('${safeId}', '${jsFile}')"
                            style="padding: 5px 10px; font-size: 11px; color: #f44336;" title="Löschen">
                        <i class="bi bi-trash"></i>
                    </button>
                </div>
            </div>
        `;
    }).join('');
}

function toggleServerContentSelection(filename) {
    if (selectedServerContent.has(filename)) {
        selectedServerContent.delete(filename);
    } else {
        selectedServerContent.add(filename);
    }
    updateServerContentBulkBar();
}

function toggleSelectAllServerContent() {
    const selectAll = document.getElementById('server-content-select-all');
    const checkboxes = document.querySelectorAll('.server-content-checkbox');
    checkboxes.forEach(cb => {
        cb.checked = !!selectAll?.checked;
        if (selectAll?.checked) selectedServerContent.add(cb.dataset.filename);
    });
    if (!selectAll?.checked) selectedServerContent.clear();
    updateServerContentBulkBar();
}

function updateServerContentBulkBar() {
    const countEl = document.getElementById('server-content-selected-count');
    if (countEl) countEl.textContent = `${selectedServerContent.size} ausgewählt`;
    const selectAll = document.getElementById('server-content-select-all');
    if (selectAll && selectedServerContent.size === 0) selectAll.checked = false;
}

async function toggleServerContentItem(serverId, filename, isDisabled) {
    try {
        await invoke('toggle_server_content', { serverId, filenames: [filename], enable: isDisabled });
        showToast(isDisabled ? 'Aktiviert' : 'Deaktiviert', 'success', 1800);
        await loadServerInstalledContent(serverId);
    } catch (error) {
        showToast('Umschalten fehlgeschlagen: ' + error, 'error', 3500);
    }
}

async function deleteServerContentItem(serverId, filename) {
    const item = serverInstalledContentCache.find(i => i.filename === filename);
    const confirmed = await showConfirmModal({
        title: 'Löschen?',
        message: 'Möchtest du ',
        highlight: item?.name || filename,
        suffix: ' wirklich löschen?',
        details: 'Die Datei wird aus dem Server-Ordner entfernt.',
    });
    if (!confirmed) return;
    try {
        await invoke('delete_server_content', { serverId, filenames: [filename] });
        showToast(`${filename} gelöscht`, 'success', 2200);
        await loadServerInstalledContent(serverId);
    } catch (error) {
        showToast('Löschen fehlgeschlagen: ' + error, 'error', 3500);
    }
}

async function bulkToggleServerContent(serverId, enable) {
    if (selectedServerContent.size === 0) {
        showToast('Bitte wähle zuerst Einträge aus', 'warning', 2500);
        return;
    }
    const count = selectedServerContent.size;
    try {
        await invoke('toggle_server_content', { serverId, filenames: Array.from(selectedServerContent), enable });
        showToast(`${count} ${enable ? 'aktiviert' : 'deaktiviert'}`, 'success', 2500);
        await loadServerInstalledContent(serverId);
    } catch (error) {
        showToast('Fehler: ' + error, 'error', 3500);
    }
}

async function bulkDeleteServerContent(serverId) {
    if (selectedServerContent.size === 0) {
        showToast('Bitte wähle zuerst Einträge aus', 'warning', 2500);
        return;
    }
    const count = selectedServerContent.size;
    const confirmed = await showConfirmModal({
        title: 'Löschen?',
        message: 'Möchtest du ',
        highlight: `${count} ausgewählte Dateien`,
        suffix: ' wirklich löschen?',
        details: 'Die Dateien werden aus dem Server-Ordner entfernt.',
    });
    if (!confirmed) return;
    try {
        await invoke('delete_server_content', { serverId, filenames: Array.from(selectedServerContent) });
        showToast(`${count} gelöscht`, 'success', 2500);
        await loadServerInstalledContent(serverId);
    } catch (error) {
        showToast('Fehler beim Löschen: ' + error, 'error', 3500);
    }
}

async function checkServerContentUpdates(serverId) {
    const panel = document.getElementById('server-content-updates');
    if (!panel) return;
    panel.style.display = 'block';
    panel.innerHTML = `<div class="server-updates-panel"><div class="spinner" style="margin: 0 auto 10px;"></div><p style="text-align:center; margin:0; color:var(--text-secondary);">Suche nach Updates...</p></div>`;

    try {
        const updates = await invoke('check_server_content_updates', { serverId });
        if (!updates.length) {
            panel.innerHTML = `
                <div class="server-updates-panel" style="display:flex; align-items:center; justify-content:space-between; gap:10px;">
                    <span><i class="bi bi-check-circle-fill" style="color:#4caf50;"></i> Alles ist auf dem neuesten Stand</span>
                    <button class="btn btn-secondary" style="padding:5px 10px; font-size:11px;" onclick="document.getElementById('server-content-updates').style.display='none'">×</button>
                </div>`;
            return;
        }

        const updatedFiles = new Set(updates.map(u => u.filename));
        serverInstalledContentCache.forEach(item => { item.has_update = updatedFiles.has(item.filename); });
        renderServerInstalledContent(serverInstalledContentCache, serverId);

        window._serverContentUpdates = updates;
        panel.innerHTML = `
            <div class="server-updates-panel">
                <div style="display:flex; align-items:center; justify-content:space-between; gap:10px; margin-bottom:8px;">
                    <strong style="color:var(--gold);">${updates.length} Update${updates.length === 1 ? '' : 's'} verfügbar</strong>
                    <div style="display:flex; gap:6px;">
                        <button class="btn" style="padding:6px 12px; font-size:12px;" onclick="applyServerContentUpdates('${escapeAttr(serverId)}')"><i class="bi bi-download"></i> Alle updaten</button>
                        <button class="btn btn-secondary" style="padding:6px 10px; font-size:12px;" onclick="document.getElementById('server-content-updates').style.display='none'">×</button>
                    </div>
                </div>
                ${updates.map((u, index) => `
                    <div class="server-update-row">
                        <span style="flex:1; min-width:0; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;">${escapeHtml(u.name)}</span>
                        <span style="color:var(--text-secondary); font-size:12px;">${escapeHtml(u.current_version || '?')} → <span style="color:var(--gold);">${escapeHtml(u.latest_version)}</span></span>
                        <button class="btn btn-secondary" style="padding:4px 10px; font-size:11px;" onclick="applyServerContentUpdates('${escapeAttr(serverId)}', ${index})">Update</button>
                    </div>
                `).join('')}
            </div>
        `;
    } catch (error) {
        panel.innerHTML = `<div class="server-updates-panel" style="color:#f44336;">Update-Prüfung fehlgeschlagen: ${escapeHtml(String(error))}</div>`;
    }
}

async function applyServerContentUpdates(serverId, onlyIndex = null) {
    const all = window._serverContentUpdates || [];
    const updates = onlyIndex === null ? all : [all[onlyIndex]].filter(Boolean);
    if (!updates.length) return;

    let success = 0;
    for (const update of updates) {
        try {
            await invoke('install_server_content', {
                serverId,
                projectId: update.project_id,
                versionId: update.latest_version_id,
            });
            success += 1;
        } catch (error) {
            showToast(`${update.name}: ${error}`, 'error', 4000);
        }
    }

    showToast(`${success}/${updates.length} aktualisiert`, success === updates.length ? 'success' : 'warning', 3000);
    await loadServerInstalledContent(serverId);
    await checkServerContentUpdates(serverId);
}

async function loadServerInstalledProjectIds(force = false) {
    if (!serverBrowserContext) return;
    const serverId = serverBrowserContext.serverId;
    if (!force && serverInstalledProjectIdsServerId === serverId) return;
    try {
        const items = await invoke('get_server_installed_content', { serverId });
        serverInstalledContentCache = items || [];
        serverInstalledProjectIds = new Set(serverInstalledContentCache.map(i => i.project_id).filter(Boolean));
        serverInstalledProjectIdsServerId = serverId;
    } catch (error) {
        debugLog('Could not load installed server content: ' + error, 'error');
    }
}

async function createServerBrowserContext(serverId) {
    const server = serverProfiles.find(s => s.id === serverId) || currentServerProfile;
    const context = await invoke('get_server_content_context', { serverId });
    return {
        serverId,
        serverName: server?.name || 'Server',
        software: server?.software || '',
        contentType: context.content_type,
        projectType: context.project_type,
        loaders: context.loaders || [],
        minecraftVersion: context.minecraft_version,
    };
}

async function openServerContentBrowser(serverId) {
    try {
        const context = await createServerBrowserContext(serverId);
        if (context.contentType === 'none') {
            showToast('Vanilla-Server unterstützen keine Plugins oder Mods', 'warning', 3000);
            return;
        }

        recordNavigation({ type: 'browser', context: 'server', serverId });
        serverBrowserContext = context;
        serverInstalledProjectIdsServerId = null;
        browserResultsCache.clear();
        currentProfile = null;
        clearServerHubIntervals();

        serverBrowserNavigation = true;
        switchPage('mods');
        browserViewKey = `browser:server:${serverId}`;
        applyServerBrowserMode();
        switchContentType('mods', { force: true });
    } catch (error) {
        showToast('Content Browser konnte nicht geöffnet werden: ' + error, 'error', 3500);
    }
}

function applyServerBrowserMode() {
    const ctx = serverBrowserContext;
    if (!ctx) return;
    const isPlugin = ctx.projectType === 'plugin';

    document.querySelectorAll('[data-content-type]').forEach(btn => {
        btn.style.display = btn.dataset.contentType === 'mods' ? '' : 'none';
    });
    const modsBtn = document.querySelector('[data-content-type="mods"]');
    if (modsBtn) {
        if (browserModeBackup.modsBtnText === null) browserModeBackup.modsBtnText = modsBtn.innerHTML;
        modsBtn.innerHTML = isPlugin ? 'Plugins' : 'Mods';
    }

    const loaderSelect = document.getElementById('filter-loader');
    if (loaderSelect) {
        if (browserModeBackup.loaderOptions === null) browserModeBackup.loaderOptions = loaderSelect.innerHTML;
        const names = ctx.loaders.map(l => SERVER_SOFTWARE[l]?.label || (l.charAt(0).toUpperCase() + l.slice(1)));
        loaderSelect.innerHTML = `<option value="" title="${escapeAttr(names.join(', '))}">Alle kompatiblen</option>` +
            ctx.loaders.map((l, i) => `<option value="${escapeAttr(l)}">${escapeHtml(names[i])}</option>`).join('');
        loaderSelect.value = '';
    }
    const loaderHeader = document.querySelector('#loader-filter-section .filter-header span');
    if (loaderHeader) loaderHeader.textContent = isPlugin ? 'Plattform' : 'Mod Loader';

    const envSection = document.getElementById('environment-filter-section');
    if (envSection) envSection.style.display = 'none';

    const versionFilter = document.getElementById('filter-version');
    if (versionFilter && ctx.minecraftVersion) {
        if (![...versionFilter.options].some(o => o.value === ctx.minecraftVersion)) {
            const option = document.createElement('option');
            option.value = ctx.minecraftVersion;
            option.textContent = ctx.minecraftVersion;
            versionFilter.appendChild(option);
        }
        versionFilter.value = ctx.minecraftVersion;
    }

    selectedFilters.loader = '';
    selectedFilters.version = ctx.minecraftVersion || '';
    selectedFilters.categories = [];

    const searchInput = document.getElementById('mod-search');
    if (searchInput) searchInput.placeholder = isPlugin ? 'Search plugins...' : 'Search mods...';
}

function restoreDefaultBrowserMode() {
    document.querySelectorAll('[data-content-type]').forEach(btn => {
        btn.style.display = '';
    });
    const modsBtn = document.querySelector('[data-content-type="mods"]');
    if (modsBtn && browserModeBackup.modsBtnText !== null) modsBtn.innerHTML = browserModeBackup.modsBtnText;

    const loaderSelect = document.getElementById('filter-loader');
    if (loaderSelect && browserModeBackup.loaderOptions !== null) {
        loaderSelect.innerHTML = browserModeBackup.loaderOptions;
        loaderSelect.value = '';
    }
    const loaderHeader = document.querySelector('#loader-filter-section .filter-header span');
    if (loaderHeader) loaderHeader.textContent = typeof t === 'function' ? t('mod_loader') : 'Mod Loader';

    const envSection = document.getElementById('environment-filter-section');
    if (envSection) envSection.style.display = '';

    browserModeBackup.modsBtnText = null;
    browserModeBackup.loaderOptions = null;
    selectedFilters.loader = '';
    browserResultsCache.clear();
    // Die angezeigten Ergebnisse gehören zu keinem Kontext mehr
    browserViewKey = null;
}

async function searchServerContent(query, page = 0) {
    const ctx = serverBrowserContext;
    const modList = document.getElementById('mod-list');
    if (!ctx || !modList) return;

    currentModSearchQuery = query || '';
    currentModPage = page;
    const label = ctx.projectType === 'plugin' ? 'Plugins' : 'Mods';
    modList.innerHTML = `<div class="loading"><div class="spinner" style="margin: 20px auto;"></div><p>Lade ${label}...</p></div>`;

    await loadServerInstalledProjectIds();

    const cacheKey = buildBrowserCacheKey(`server-${ctx.serverId}`, query || '', page);
    const cached = getCachedBrowserResults(cacheKey);
    if (cached) {
        renderMods(cached, page);
        return;
    }

    try {
        const results = await invoke('search_server_content', {
            projectType: ctx.projectType,
            query: query || '',
            gameVersion: selectedFilters.version || null,
            loaders: selectedFilters.loader ? [selectedFilters.loader] : ctx.loaders,
            categories: selectedFilters.categories.length > 0 ? selectedFilters.categories : null,
            sortBy: query ? (selectedFilters.sort || 'relevance') : (selectedFilters.sort || 'downloads'),
            offset: page * getEffectiveLimit(),
            limit: getEffectiveLimit(),
        });
        // Nutzer könnte den Browser inzwischen verlassen haben
        if (serverBrowserContext !== ctx) return;
        setCachedBrowserResults(cacheKey, results);
        renderMods(results, page);
    } catch (error) {
        modList.innerHTML = `<div class="loading">Suche fehlgeschlagen: ${escapeHtml(String(error))}</div>`;
    }
}

async function installServerContentFromBrowser(projectId, _source) {
    const ctx = serverBrowserContext;
    if (!ctx) return;
    const btn = event?.target?.closest('button');
    const originalText = btn?.innerHTML;
    if (btn) {
        btn.textContent = '...';
        btn.disabled = true;
    }

    try {
        const result = await invoke('install_server_content', {
            serverId: ctx.serverId,
            projectId,
            versionId: null,
        });
        if (btn) {
            btn.innerHTML = '<i class="bi bi-check"></i> Installiert';
            btn.style.background = 'var(--bg-light)';
            btn.style.color = 'var(--text-secondary)';
            btn.style.opacity = '0.7';
            btn.style.cursor = 'not-allowed';
        }
        await loadServerInstalledProjectIds(true);
        browserResultsCache.clear();

        const extra = result.installed.length > 1 ? ` (inkl. ${result.installed.slice(1).join(', ')})` : '';
        showToast(`${result.installed[0] || 'Inhalt'} zu "${ctx.serverName}" hinzugefügt${extra}`, 'success', 3500);
        (result.warnings || []).forEach(w => showToast(w, 'warning', 5000));
    } catch (error) {
        if (btn) {
            btn.innerHTML = originalText;
            btn.disabled = false;
        }
        showToast('Installation fehlgeschlagen: ' + error, 'error', 4500);
    }
}

async function showServerContentDetails(serverId, projectId) {
    try {
        serverBrowserContext = await createServerBrowserContext(serverId);
        clearServerHubIntervals();
        currentProfile = null;
        await showModDetails(projectId, 'modrinth');
    } catch (error) {
        showToast('Details konnten nicht geladen werden: ' + error, 'error', 3500);
    }
}

// ==================== SERVER CONSOLE ====================

async function loadServerConsole(serverId) {
    const el = document.getElementById('server-console-output');
    if (!el) return;

    const shouldStickToBottom = serverConsoleAutoScroll
        || Math.abs((el.scrollHeight - el.scrollTop) - el.clientHeight) < 20;
    try {
        const output = await invoke('get_server_console_output', { serverId });
        if (el.textContent !== (output || '')) {
            el.textContent = output || '';
        }
        if (shouldStickToBottom) {
            el.scrollTop = el.scrollHeight;
        }
    } catch (error) {
        el.textContent = 'Failed to read console output: ' + error;
    }
}

function clearServerConsoleOutput() {
    const el = document.getElementById('server-console-output');
    if (el) el.textContent = '';
}

async function sendServerConsoleCommand(serverId) {
    const input = document.getElementById('server-console-command');
    const command = input?.value.trim() || '';
    if (!command) return;

    try {
        await invoke('send_server_console_command', { serverId, command });
        if (input) input.value = '';
        await loadServerConsole(serverId);
    } catch (error) {
        showToast('Command konnte nicht gesendet werden: ' + error, 'error', 3000);
    }
}

// ==================== SERVER BACKUPS ====================

function renderServerBackupsTab(server) {
    const safeId = escapeAttr(server.id);
    return `
        <div class="server-panel">
            <div style="display:flex; justify-content:space-between; align-items:center; gap:10px; flex-wrap:wrap; margin-bottom:12px;">
                <div>
                    <h3 style="color: var(--gold); margin: 0;">Backups</h3>
                    <div style="font-size:12px; color:var(--text-secondary); margin-top:4px;">
                        Läuft der Server, wird das automatische Speichern während des Backups kurz pausiert.
                    </div>
                </div>
                <div style="display:flex; gap:8px; flex-wrap:wrap;">
                    <button class="btn" data-backup-action onclick="createServerBackup('${safeId}', false)"><i class="bi bi-archive"></i> Backup erstellen</button>
                    <button class="btn btn-secondary" data-backup-action onclick="createServerBackup('${safeId}', true)"><i class="bi bi-globe"></i> Nur Welten</button>
                    <button class="btn btn-secondary" onclick="loadServerBackups('${safeId}')" title="Aktualisieren"><i class="bi bi-arrow-clockwise"></i></button>
                    <button class="btn btn-secondary" onclick="openServerFolder('${safeId}', 'backups')" title="Ordner öffnen"><i class="bi bi-folder"></i></button>
                </div>
            </div>
            <div id="server-backup-status"></div>
            <div id="server-backups-list" class="profile-scroll-box" style="display:grid; gap:8px; align-content:start; max-height:460px; overflow-y:auto; padding-right:5px;">
                <div style="text-align:center; padding:40px; color:var(--text-secondary);"><div class="spinner" style="margin:0 auto 15px;"></div><p>Lade Backups...</p></div>
            </div>
        </div>
    `;
}

async function loadServerBackups(serverId) {
    const list = document.getElementById('server-backups-list');
    if (!list) return;

    try {
        const backups = await invoke('list_server_backups', { serverId });
        if (!backups.length) {
            list.innerHTML = `
                <div style="text-align:center; padding:50px 20px; color:var(--text-secondary);">
                    <div style="font-size:44px; margin-bottom:12px;"><i class="bi bi-archive"></i></div>
                    <p style="margin:0;">Noch keine Backups vorhanden</p>
                </div>`;
            return;
        }

        const safeId = escapeAttr(serverId);
        list.innerHTML = backups.map(backup => {
            const name = escapeAttr(backup.file_name);
            const isWorlds = backup.kind === 'worlds';
            return `
                <div class="server-backup-row">
                    <div class="server-content-icon"><i class="bi ${isWorlds ? 'bi-globe' : 'bi-archive-fill'}" style="font-size:20px; color:var(--gold);"></i></div>
                    <div style="flex:1; min-width:0;">
                        <div style="display:flex; align-items:center; gap:8px; flex-wrap:wrap;">
                            <strong style="font-size:14px;">${new Date(backup.created_unix * 1000).toLocaleString()}</strong>
                            <span class="server-content-tag">${isWorlds ? 'Welten' : 'Komplett'}</span>
                        </div>
                        <div style="font-size:11px; color:var(--text-secondary); margin-top:3px; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;">
                            ${escapeHtml(backup.file_name)} • ${formatBytes(backup.size || 0)}
                        </div>
                    </div>
                    <div style="display:flex; gap:6px; flex-shrink:0;">
                        <button class="btn btn-secondary" data-backup-action style="padding:6px 12px; font-size:12px;" onclick="restoreServerBackup('${safeId}', '${name}')"><i class="bi bi-arrow-counterclockwise"></i> Wiederherstellen</button>
                        <button class="btn btn-secondary" data-backup-action style="padding:6px 10px; font-size:12px; color:#f44336;" onclick="deleteServerBackup('${safeId}', '${name}')" title="Löschen"><i class="bi bi-trash"></i></button>
                    </div>
                </div>
            `;
        }).join('');
        setServerBackupBusy(serverBackupBusy);
    } catch (error) {
        list.innerHTML = `<div style="color:#f44336; padding:20px;">Backups konnten nicht geladen werden: ${escapeHtml(String(error))}</div>`;
    }
}

function setServerBackupBusy(busy, message = '') {
    serverBackupBusy = busy;
    document.querySelectorAll('[data-backup-action]').forEach(btn => { btn.disabled = busy; });
    const status = document.getElementById('server-backup-status');
    if (status) {
        status.innerHTML = busy
            ? `<div class="server-updates-panel" style="display:flex; align-items:center; gap:10px;"><div class="spinner" style="width:18px; height:18px; margin:0;"></div><span>${escapeHtml(message)}</span></div>`
            : '';
    }
}

async function createServerBackup(serverId, worldsOnly) {
    if (serverBackupBusy) return;
    setServerBackupBusy(true, worldsOnly ? 'Welten werden gesichert...' : 'Backup wird erstellt – das kann bei großen Welten etwas dauern...');
    try {
        const backup = await invoke('create_server_backup', { serverId, worldsOnly });
        showToast(`Backup erstellt (${formatBytes(backup.size || 0)})`, 'success', 3000);
    } catch (error) {
        showToast('Backup fehlgeschlagen: ' + error, 'error', 4500);
    } finally {
        setServerBackupBusy(false);
        loadServerBackups(serverId);
    }
}

async function restoreServerBackup(serverId, fileName) {
    if (serverBackupBusy) return;
    const worlds = fileName.toLowerCase().endsWith('-worlds.zip');
    const warning = worlds
        ? 'Die im Backup enthaltenen Welten werden durch den Stand des Backups ersetzt.'
        : 'Alle Serverdateien (außer Backups) werden durch den Stand des Backups ersetzt.';
    const confirmed = await showConfirmModal({
        title: 'Backup wiederherstellen?',
        message: 'Möchtest du das Backup ',
        highlight: fileName,
        suffix: ' wirklich wiederherstellen?',
        details: warning,
        confirmText: 'Wiederherstellen',
        icon: 'bi-arrow-counterclockwise',
    });
    if (!confirmed) return;

    setServerBackupBusy(true, 'Backup wird wiederhergestellt...');
    try {
        await invoke('restore_server_backup', { serverId, fileName });
        showToast('Backup wiederhergestellt', 'success', 3000);
    } catch (error) {
        showToast('Wiederherstellen fehlgeschlagen: ' + error, 'error', 5000);
    } finally {
        setServerBackupBusy(false);
        loadServerBackups(serverId);
    }
}

async function deleteServerBackup(serverId, fileName) {
    const confirmed = await showConfirmModal({
        title: 'Backup löschen?',
        message: 'Möchtest du das Backup ',
        highlight: fileName,
        suffix: ' endgültig löschen?',
    });
    if (!confirmed) return;
    try {
        await invoke('delete_server_backup', { serverId, fileName });
        showToast('Backup gelöscht', 'success', 2000);
        loadServerBackups(serverId);
    } catch (error) {
        showToast('Löschen fehlgeschlagen: ' + error, 'error', 3500);
    }
}

// ==================== SERVER SETTINGS ====================

async function loadServerSettings(serverId) {
    const shell = document.getElementById('server-settings-content');
    if (!shell) return;

    try {
        const profile = serverProfiles.find(s => s.id === serverId);
        const response = await invoke('get_server_properties', { serverId });
        const values = response.exists ? (response.values || {}) : {};
        const bool = (key, def = 'false') => (values[key] ?? def) === 'true' ? 'checked' : '';
        const javaArgsRaw = Array.isArray(profile?.java_args) && profile.java_args.length > 0
            ? profile.java_args.join(' ')
            : DEFAULT_SERVER_JAVA_ARGS;
        const memoryMb = profile?.memory_mb || 4096;
        const port = Number(profile?.port || values['server-port'] || 25565);
        const autoRestart = profile?.auto_restart ? 'checked' : '';

        shell.innerHTML = `
            ${response.exists ? '' : '<div style="margin-bottom:10px; padding:8px 10px; border-radius:8px; background:var(--bg-light); color:var(--text-secondary); font-size:12px;">server.properties wird beim ersten Start automatisch erstellt. Du kannst die Werte trotzdem schon konfigurieren.</div>'}
            <div class="server-settings-panel">
                <h3 style="margin:0 0 10px; color:var(--gold);">Server Launch Settings</h3>
                <div class="server-settings-grid">
                    <label class="server-field">
                        <span>Memory (MB)</span>
                        <input class="server-input" id="srv-memory" type="number" min="512" max="65536" step="256" value="${escapeAttr(String(memoryMb))}">
                    </label>
                    <label class="server-field">
                        <span>Port</span>
                        <input class="server-input" id="srv-port" type="number" min="1" max="65535" value="${escapeAttr(String(port || 25565))}">
                    </label>
                    <div class="server-toggle-row">
                        <span>Auto Restart (bei Crash)</span>
                        <label class="switch">
                            <input id="srv-auto-restart" type="checkbox" ${autoRestart}>
                            <span class="switch-slider"></span>
                        </label>
                    </div>
                    <label class="server-field server-field-wide">
                        <span>Java Flags</span>
                        <textarea class="server-textarea" id="srv-java-args">${escapeHtml(javaArgsRaw)}</textarea>
                    </label>
                </div>
            </div>

            <div class="server-settings-panel" style="margin-top:14px;">
                <h3 style="margin:0 0 10px; color:var(--gold);">server.properties</h3>
                <div class="server-settings-grid">
                    <div class="server-toggle-row">
                        <span>Online Mode</span>
                        <label class="switch">
                            <input id="srv-online-mode" type="checkbox" ${bool('online-mode', 'true')}>
                            <span class="switch-slider"></span>
                        </label>
                    </div>
                    <div class="server-toggle-row">
                        <span>PvP</span>
                        <label class="switch">
                            <input id="srv-pvp" type="checkbox" ${bool('pvp', 'true')}>
                            <span class="switch-slider"></span>
                        </label>
                    </div>
                    <div class="server-toggle-row">
                        <span>Allow Flight</span>
                        <label class="switch">
                            <input id="srv-allow-flight" type="checkbox" ${bool('allow-flight', 'false')}>
                            <span class="switch-slider"></span>
                        </label>
                    </div>
                    <div class="server-toggle-row">
                        <span>Command Blocks</span>
                        <label class="switch">
                            <input id="srv-command-block" type="checkbox" ${bool('enable-command-block', 'false')}>
                            <span class="switch-slider"></span>
                        </label>
                    </div>
                    <label class="server-field">
                        <span>View Distance</span>
                        <input class="server-input" id="srv-view-distance" type="number" min="2" max="32" value="${escapeAttr(values['view-distance'] || '10')}">
                    </label>
                    <label class="server-field">
                        <span>Max Players</span>
                        <input class="server-input" id="srv-max-players" type="number" min="1" max="1000" value="${escapeAttr(values['max-players'] || '20')}">
                    </label>
                    <label class="server-field">
                        <span>Server Port</span>
                        <input class="server-input" id="srv-prop-port" type="number" min="1" max="65535" value="${escapeAttr(values['server-port'] || String(port || 25565))}">
                    </label>
                    <label class="server-field">
                        <span>Difficulty</span>
                        <select class="server-select" id="srv-difficulty">
                            <option value="peaceful" ${(values.difficulty || 'easy') === 'peaceful' ? 'selected' : ''}>Peaceful</option>
                            <option value="easy" ${(values.difficulty || 'easy') === 'easy' ? 'selected' : ''}>Easy</option>
                            <option value="normal" ${(values.difficulty || 'easy') === 'normal' ? 'selected' : ''}>Normal</option>
                            <option value="hard" ${(values.difficulty || 'easy') === 'hard' ? 'selected' : ''}>Hard</option>
                        </select>
                    </label>
                    <label class="server-field">
                        <span>Gamemode</span>
                        <select class="server-select" id="srv-gamemode">
                            <option value="survival" ${(values.gamemode || 'survival') === 'survival' ? 'selected' : ''}>Survival</option>
                            <option value="creative" ${(values.gamemode || 'survival') === 'creative' ? 'selected' : ''}>Creative</option>
                            <option value="adventure" ${(values.gamemode || 'survival') === 'adventure' ? 'selected' : ''}>Adventure</option>
                            <option value="spectator" ${(values.gamemode || 'survival') === 'spectator' ? 'selected' : ''}>Spectator</option>
                        </select>
                    </label>
                    <label class="server-field server-field-wide">
                        <span>MOTD</span>
                        <input class="server-input" id="srv-motd" type="text" value="${escapeAttr(values.motd || 'A Lion Server')}">
                    </label>
                    <label class="server-field server-field-wide">
                        <span>Seed</span>
                        <input class="server-input" id="srv-seed" type="text" value="${escapeAttr(values['level-seed'] || '')}">
                    </label>
                </div>
            </div>
            <div style="margin-top:14px; display:flex; justify-content:flex-end;">
                <button class="btn" onclick="saveServerSettings('${serverId}')">Save Server Settings</button>
            </div>
            <div style="margin-top:18px; background:rgba(180, 34, 34, 0.14); border:1px solid #a11f1f; border-radius:10px; padding:12px;">
                <div style="color:#ff8e8e; font-weight:700; margin-bottom:6px;">Danger Zone</div>
                <div style="font-size:12px; color:#ffb2b2; margin-bottom:10px;">Serverprofil und alle Dateien werden dauerhaft geloescht.</div>
                <button class="btn" style="background:#b71c1c; color:#fff;" onclick="deleteServerProfile('${serverId}')">
                    <i class="bi bi-trash"></i> Delete Server
                </button>
            </div>
        `;
    } catch (error) {
        shell.innerHTML = `<div style="color:#f44336;">Failed to load settings: ${escapeHtml(String(error))}</div>`;
    }
}

async function saveServerSettings(serverId) {
    const values = {
        'online-mode': document.getElementById('srv-online-mode')?.checked ? 'true' : 'false',
        pvp: document.getElementById('srv-pvp')?.checked ? 'true' : 'false',
        'allow-flight': document.getElementById('srv-allow-flight')?.checked ? 'true' : 'false',
        'enable-command-block': document.getElementById('srv-command-block')?.checked ? 'true' : 'false',
        'view-distance': document.getElementById('srv-view-distance')?.value || '10',
        'max-players': document.getElementById('srv-max-players')?.value || '20',
        'server-port': document.getElementById('srv-prop-port')?.value || document.getElementById('srv-port')?.value || '25565',
        difficulty: document.getElementById('srv-difficulty')?.value || 'easy',
        gamemode: document.getElementById('srv-gamemode')?.value || 'survival',
        motd: document.getElementById('srv-motd')?.value || 'A Lion Server',
        'level-seed': document.getElementById('srv-seed')?.value || '',
    };

    const memoryRaw = parseInt(document.getElementById('srv-memory')?.value || '4096', 10);
    const memoryMb = Number.isFinite(memoryRaw) ? memoryRaw : 4096;
    const rawPort = parseInt(document.getElementById('srv-port')?.value || values['server-port'] || '25565', 10);
    const port = Number.isFinite(rawPort) && rawPort > 0 && rawPort <= 65535 ? rawPort : 25565;
    const javaArgs = document.getElementById('srv-java-args')?.value || '';
    const autoRestart = !!document.getElementById('srv-auto-restart')?.checked;

    try {
        await invoke('save_server_properties', { serverId, values });
        const updated = await invoke('update_server_profile_settings', {
            serverId,
            memoryMb,
            javaArgs,
            autoRestart,
            port,
        });

        const idx = serverProfiles.findIndex(s => s.id === serverId);
        if (idx >= 0) serverProfiles[idx] = updated;
        if (currentServerProfile?.id === serverId) currentServerProfile = updated;
        showToast('Server settings saved', 'success', 2200);
    } catch (error) {
        showToast('Failed to save settings: ' + error, 'error', 4500);
    }
}

function triggerServerFileUpload() {
    const el = document.getElementById('server-file-upload');
    if (el) el.click();
}

async function uploadServerFileFromInput(serverId, input) {
    const file = input?.files?.[0];
    if (!file) return;

    try {
        const base64 = await fileToBase64(file);
        const targetPath = currentServerFilesPath
            ? `${currentServerFilesPath}/${file.name}`
            : file.name;

        await invoke('upload_server_file', {
            serverId,
            relativePath: targetPath,
            fileBase64: base64,
        });

        await loadServerFiles(serverId, currentServerFilesPath || '');
        showToast(`${file.name} hochgeladen`, 'success', 2200);
    } catch (error) {
        showToast('Upload fehlgeschlagen: ' + error, 'error', 3800);
    } finally {
        input.value = '';
    }
}

function isLikelyTextFile(fileName) {
    const lower = String(fileName || '').toLowerCase();
    const textExt = [
        '.txt', '.log', '.json', '.yml', '.yaml', '.properties', '.cfg', '.toml',
        '.ini', '.md', '.conf', '.sh', '.bat', '.cmd', '.ps1', '.xml', '.csv', '.mcmeta',
    ];
    return textExt.some(ext => lower.endsWith(ext));
}

async function loadServerFiles(serverId, relativePath = '') {
    const list = document.getElementById('server-files-list');
    const pathEl = document.getElementById('server-files-path');
    if (!list) return;

    list.innerHTML = '<div class="loading">Loading files...</div>';

    try {
        const response = await invoke('list_server_files', {
            serverId,
            relativePath: relativePath || null,
        });

        currentServerFilesPath = response.current_path || '';
        if (pathEl) pathEl.textContent = '/' + (currentServerFilesPath || '');

        if (!response.entries || response.entries.length === 0) {
            list.innerHTML = '<div style="padding:16px; color:var(--text-secondary);">Ordner ist leer.</div>';
            return;
        }

        list.innerHTML = response.entries.map(entry => {
            const escapedPath = String(entry.relative_path)
                .replace(/\\/g, '\\\\')
                .replace(/'/g, "\\'");
            const canEdit = !entry.is_dir && isLikelyTextFile(entry.name);

            return `
                <div style="display:flex; justify-content:space-between; align-items:center; gap:10px; padding:8px 10px; margin-bottom:8px; border-radius:8px; background:var(--bg-light);">
                    <button class="btn btn-secondary" style="padding:6px 10px; min-width:42px;" onclick="openServerFileEntry('${serverId}', '${escapedPath}', ${entry.is_dir})">
                        <i class="bi ${entry.is_dir ? 'bi-folder2-open' : 'bi-file-earmark-text'}"></i>
                    </button>
                    <div style="flex:1; min-width:0;">
                        <div style="font-weight:600; overflow:hidden; text-overflow:ellipsis; white-space:nowrap;">${escapeHtml(entry.name)}</div>
                        <div style="font-size:12px; color:var(--text-secondary);">${entry.is_dir ? 'Ordner' : formatBytes(entry.size)} • ${new Date((entry.modified_unix || 0) * 1000).toLocaleString()}</div>
                    </div>
                    <div style="display:flex; gap:6px;">
                        ${canEdit ? `<button class="btn btn-secondary" style="padding:6px 10px;" onclick="openServerFileEditor('${serverId}', '${escapedPath}')"><i class='bi bi-pencil-square'></i></button>` : ''}
                        <button class="btn btn-secondary" style="padding:6px 10px;" onclick="promptMoveServerPath('${serverId}', '${escapedPath}')"><i class="bi bi-arrows-move"></i></button>
                        <button class="btn btn-secondary" style="padding:6px 10px; color:#f44336;" onclick="deleteServerPathEntry('${serverId}', '${escapedPath}', ${entry.is_dir})"><i class="bi bi-trash"></i></button>
                    </div>
                </div>
            `;
        }).join('');
    } catch (error) {
        list.innerHTML = `<div style="color:#f44336;">Dateiliste konnte nicht geladen werden: ${escapeHtml(String(error))}</div>`;
    }
}

function navigateServerFilesUp(serverId) {
    if (!currentServerFilesPath) {
        loadServerFiles(serverId, '');
        return;
    }

    const segments = currentServerFilesPath.split('/').filter(Boolean);
    segments.pop();
    const parent = segments.join('/');
    loadServerFiles(serverId, parent);
}

function openServerFileEntry(serverId, relativePath, isDir) {
    if (isDir) {
        loadServerFiles(serverId, relativePath);
        return;
    }

    if (isLikelyTextFile(relativePath)) {
        openServerFileEditor(serverId, relativePath);
    } else {
        showToast('Dateityp ist nicht direkt editierbar. Nutze "Open Extern".', 'info', 2800);
    }
}

async function openServerFileEditor(serverId, relativePath) {
    try {
        const content = await invoke('read_server_text_file', { serverId, relativePath });
        currentServerEditedFile = relativePath;

        const shell = document.getElementById('server-file-editor-shell');
        const name = document.getElementById('server-file-editor-name');
        const editor = document.getElementById('server-file-editor');

        if (shell) shell.style.display = 'block';
        if (name) name.textContent = relativePath;
        if (editor) editor.value = content || '';
    } catch (error) {
        showToast('Datei konnte nicht geoeffnet werden: ' + error, 'error', 3800);
    }
}

function closeServerFileEditor() {
    currentServerEditedFile = null;
    const shell = document.getElementById('server-file-editor-shell');
    if (shell) shell.style.display = 'none';
}

async function saveServerFileEditor(serverId) {
    if (!currentServerEditedFile) return;
    const editor = document.getElementById('server-file-editor');
    if (!editor) return;

    try {
        await invoke('write_server_text_file', {
            serverId,
            relativePath: currentServerEditedFile,
            content: editor.value,
        });
        showToast('Datei gespeichert', 'success', 2000);
        await loadServerFiles(serverId, currentServerFilesPath || '');
    } catch (error) {
        showToast('Speichern fehlgeschlagen: ' + error, 'error', 3500);
    }
}

async function deleteServerPathEntry(serverId, relativePath, isDir) {
    const confirmed = await showConfirmModal({
        title: isDir ? 'Ordner löschen?' : 'Datei löschen?',
        message: `Möchtest du ${isDir ? 'den Ordner' : 'die Datei'} `,
        highlight: relativePath,
        suffix: ' wirklich löschen?',
        details: isDir ? 'Der komplette Inhalt des Ordners wird ebenfalls gelöscht.' : '',
    });
    if (!confirmed) return;

    try {
        await invoke('delete_server_path', { serverId, relativePath });
        if (currentServerEditedFile === relativePath) {
            closeServerFileEditor();
        }
        await loadServerFiles(serverId, currentServerFilesPath || '');
    } catch (error) {
        showToast('Loeschen fehlgeschlagen: ' + error, 'error', 3500);
    }
}

async function promptMoveServerPath(serverId, fromPath) {
    const target = await showPromptModal({
        title: 'Verschieben',
        message: 'Neuer relativer Zielpfad:',
        defaultValue: fromPath,
        confirmText: 'Verschieben',
        icon: 'bi-arrows-move'
    });
    if (!target || target.trim() === '' || target === fromPath) return;

    try {
        await invoke('move_server_path', {
            serverId,
            fromPath,
            toPath: target.trim(),
        });

        if (currentServerEditedFile === fromPath) {
            currentServerEditedFile = target.trim();
        }

        await loadServerFiles(serverId, currentServerFilesPath || '');
    } catch (error) {
        showToast('Verschieben fehlgeschlagen: ' + error, 'error', 3500);
    }
}

async function createServerFolderPrompt(serverId) {
    const folderName = await showPromptModal({
        title: 'Neuer Ordner',
        message: 'Ordnername (relativ zum aktuellen Pfad):',
        defaultValue: 'new-folder',
        confirmText: 'Erstellen',
        icon: 'bi-folder-plus'
    });
    if (!folderName || !folderName.trim()) return;

    const relativePath = currentServerFilesPath
        ? `${currentServerFilesPath}/${folderName.trim()}`
        : folderName.trim();

    try {
        await invoke('create_server_folder', { serverId, relativePath });
        await loadServerFiles(serverId, currentServerFilesPath || '');
    } catch (error) {
        showToast('Ordner konnte nicht erstellt werden: ' + error, 'error', 3500);
    }
}

async function openServerFolder(serverId, subfolder = null) {
    try {
        await invoke('open_server_folder', { serverId, subfolder });
    } catch (error) {
        showToast('Failed to open folder: ' + error, 'error', 3500);
    }
}

async function deleteServerProfile(serverId) {
    const server = serverProfiles.find(s => s.id === serverId);
    const confirmed = await showConfirmModal({
        title: 'Delete server?',
        message: 'Do you really want to delete ',
        highlight: server ? server.name : 'this server',
        details: 'All server files including worlds, plugins/mods and backups will be removed permanently.',
    });
    if (!confirmed) return;
    try {
        clearServerHubIntervals();
        await invoke('delete_server_profile', { serverId });
        currentServerProfile = null;
        await loadServerHub();
        showToast('Server deleted', 'success', 2000);
    } catch (error) {
        showToast('Failed to delete server: ' + error, 'error', 3500);
    }
}

// Helper: Formatiert Bytes in lesbare Größe
function formatBytes(bytes) {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
}

// Helper: Formatiert Timestamp in lesbares Datum
function formatTimestamp(timestamp) {
    // Minecraft speichert LastPlayed in Millisekunden
    const date = new Date(timestamp);
    if (isNaN(date.getTime())) return 'Unbekannt';

    const now = new Date();
    const diff = now - date;

    // Relative Zeit
    if (diff < 60000) return 'Gerade eben';
    if (diff < 3600000) return `Vor ${Math.floor(diff / 60000)} Min.`;
    if (diff < 86400000) return `Vor ${Math.floor(diff / 3600000)} Std.`;
    if (diff < 604800000) return `Vor ${Math.floor(diff / 86400000)} Tagen`;

    // Absolutes Datum
    return date.toLocaleDateString('de-DE', { day: '2-digit', month: '2-digit', year: 'numeric' });
}

// Profil-Bild Vorschau
let selectedProfileIcon = null;

function previewProfileIcon(event) {
    const file = event.target.files[0];
    if (!file) return;

    const reader = new FileReader();
    reader.onload = function(e) {
        selectedProfileIcon = e.target.result;
        const preview = document.getElementById('profile-icon-preview');
        if (preview) {
            preview.innerHTML = `<img alt="" src="${e.target.result}" style="width: 100%; height: 100%; object-fit: cover;">`;
        }
    };
    reader.readAsDataURL(file);
}

function clearProfileIcon() {
    selectedProfileIcon = null;
    const preview = document.getElementById('profile-icon-preview');
    if (preview) {
        preview.innerHTML = '<i class="bi bi-box"></i>';
    }
    const input = document.getElementById('profile-icon-input');
    if (input) input.value = '';
}

// Versionen im Edit-Dialog laden
async function populateEditVersionSelect() {
    const select = document.getElementById('edit-profile-mc-version');
    if (!select) return;

    // Falls Versionen noch nicht geladen, lade sie
    if (!allMinecraftVersions || allMinecraftVersions.length === 0) {
        try {
            allMinecraftVersions = await invoke('get_minecraft_versions');
        } catch (e) {
            console.error('Failed to load versions:', e);
            return;
        }
    }

    // Benutzerwahl beibehalten statt immer auf den Profilwert zurückzusetzen
    const currentVersion = select.value || currentProfile?.minecraft_version || '';
    const showSnapshotsEdit = document.getElementById('edit-show-snapshots')?.checked || false;

    // Prüfe welcher Loader gewählt ist
    const loaderSelect = document.getElementById('edit-profile-loader');
    const selectedLoader = loaderSelect ? loaderSelect.value : currentProfile?.loader?.loader || 'vanilla';

    let filtered = allMinecraftVersions.filter(v => {
        if (showSnapshotsEdit) return true;
        return (v.version_type || '').toLowerCase() === 'release';
    });

    // Für jeden Loader: Nur MC-Versionen anzeigen die tatsächlich unterstützt werden
    filtered = await filterVersionsByLoader(filtered, selectedLoader);

    const hasCurrentVersion = !!currentVersion && filtered.some(v => v.id === currentVersion);
    const fallbackOption = (!hasCurrentVersion && currentVersion)
        ? [`<option value="${currentVersion}" selected>${currentVersion} (aktuell)</option>`]
        : [];

    const options = filtered.map(v =>
        `<option value="${v.id}" ${v.id === currentVersion ? 'selected' : ''}>${v.id}</option>`
    );

    select.innerHTML = [...fallbackOption, ...options].join('');

    // Wert explizit setzen, damit Browser nicht auf die erste Option springt.
    if (currentVersion) {
        select.value = currentVersion;
    }
}

async function updateEditVersionList() {
    await populateEditVersionSelect();
    await updateEditLoaderVersions();
}

async function updateEditLoaderVersions() {
    const loaderSelect = document.getElementById('edit-profile-loader');
    const versionSelect = document.getElementById('edit-profile-loader-version');
    const mcVersionSelect = document.getElementById('edit-profile-mc-version');

    if (!loaderSelect || !versionSelect) return;

    const loader = loaderSelect.value;
    const desiredVersion = versionSelect.value || currentProfile?.loader?.version || '';

    // WICHTIG: Aktualisiere verfügbare MC-Versionen basierend auf Loader!
    // (z.B. NeoForge nur 1.20.2+)
    await populateEditVersionSelect();

    // Nach dem Rebuild immer die tatsächlich ausgewählte MC-Version verwenden.
    const mcVersion = mcVersionSelect?.value || currentProfile?.minecraft_version;

    if (loader === 'vanilla') {
        versionSelect.innerHTML = '<option value="">-</option>';
        versionSelect.disabled = true;
        return;
    }

    versionSelect.disabled = false;
    versionSelect.innerHTML = '<option value="">Lade...</option>';

    try {
        let versions = [];

        if (loader === 'fabric') {
            versions = await invoke('get_fabric_versions', { minecraftVersion: mcVersion });
        } else if (loader === 'quilt') {
            versions = await invoke('get_quilt_versions', { minecraftVersion: mcVersion });
        } else if (loader === 'forge') {
            versions = await invoke('get_forge_versions', { minecraftVersion: mcVersion });
        } else if (loader === 'neoforge') {
            versions = await invoke('get_neoforge_versions', { minecraftVersion: mcVersion });
        }

        if (versions && versions.length > 0) {
            // Zeige die neueste als Standard + alle anderen
            versionSelect.innerHTML = '<option value="">Neueste (' + versions[0] + ')</option>' +
                versions.map(v => `<option value="${v}">${v}</option>`).join('');

            // Gespeicherte konkrete Loader-Version wieder selektieren (falls vorhanden)
            if (desiredVersion && versions.includes(desiredVersion)) {
                versionSelect.value = desiredVersion;
            } else {
                versionSelect.value = '';
            }
        } else {
            versionSelect.innerHTML = '<option value="">Neueste</option>';
            versionSelect.value = '';
        }
    } catch (error) {
        console.error('Failed to load loader versions:', error);
        if (loader === 'quilt') {
            versionSelect.innerHTML = '<option value="">Neueste (kompatibler Loader)</option>';
            showToast('Quilt hat für diese MC-Version noch keine offizielle Unterstützung – der Launcher nutzt automatisch den kompatiblen Fallback-Loader.', 'info', 6000);
        } else {
            versionSelect.innerHTML = '<option value="">Neueste (Fehler beim Laden)</option>';
        }
        versionSelect.value = '';
    }
}

async function saveProfileSettings(profileId) {
    const nameInput = document.getElementById('edit-profile-name');
    const memoryInput = document.getElementById('edit-profile-memory');
    const mcVersionSelect = document.getElementById('edit-profile-mc-version');
    const loaderSelect = document.getElementById('edit-profile-loader');
    const loaderVersionSelect = document.getElementById('edit-profile-loader-version');
    const javaArgsTextarea = document.getElementById('edit-profile-java-args');

    if (!nameInput || !memoryInput) {
        debugLog('Form elements not found', 'error');
        return;
    }

    try {
        debugLog('Saving profile settings...', 'info');

        const updates = {
            name: nameInput.value,
            minecraft_version: mcVersionSelect?.value || currentProfile.minecraft_version,
            loader: loaderSelect?.value || currentProfile.loader.loader,
            loader_version: loaderVersionSelect?.value || currentProfile.loader.version,
            memory_mb: parseInt(memoryInput.value) || 4096,
            java_args: javaArgsTextarea?.value.split(' ').filter(a => a.trim()) || [],
            icon_path: selectedProfileIcon || currentProfile.icon_path
        };

        await invoke('update_profile', {
            profileId: profileId,
            updates: updates
        });

        showToast('Profil-Einstellungen gespeichert!', 'success', 3000);
        selectedProfileIcon = null;

        // Reload profiles
        await loadProfiles();

        // Zeige aktualisiertes Profil
        const updatedProfile = profiles.find(p => p.id === profileId);
        if (updatedProfile) {
            showProfileDetails(profileId);
        }
    } catch (error) {
        debugLog('Failed to save settings: ' + error, 'error');
        showToast('Fehler beim Speichern: ' + error, 'error', 5000);
    }
}

// Modals
function openCreateProfileModal() {
    const modal = document.getElementById('create-profile-modal');
    if (modal) {
        modal.classList.add('active');
        // Versions laden und danach sofort Loader-Versionen für die gewählte Kombination
        updateVersionSelects().then(() => updateCreateLoaderVersions());
    }
}

function setupModals() {
    const createBtn = document.getElementById('create-profile-btn');
    if (createBtn) {
        createBtn.addEventListener('click', openCreateProfileModal);
    }

    const cancelBtn = document.getElementById('cancel-profile-btn');
    const saveBtn = document.getElementById('save-profile-btn');
    const importBtn = document.getElementById('import-profile-btn');
    const lionImportInput = document.getElementById('lion-import-input');
    const loaderSelect = document.getElementById('profile-loader');
    const loaderWarning = document.getElementById('loader-warning');

    if (importBtn) {
        importBtn.addEventListener('click', (e) => {
            e.preventDefault();
            openLionImportDialog();
        });
    }

    if (lionImportInput) {
        lionImportInput.addEventListener('change', handleLionImportInputChange);
    }

    // Hilfsfunktion: Warnung nur für Quilt 0.26.1.x anzeigen
    function checkQuilt261Warning() {
        const loader = loaderSelect ? loaderSelect.value : '';
        const loaderVersionEl = document.getElementById('profile-loader-version');
        let selectedVersion = loaderVersionEl ? loaderVersionEl.value : '';

        // Wenn "Neueste" (value="") gewählt ist, Versionsstring aus dem Optionstext lesen
        // Der Text lautet z.B. "Neueste (0.26.1-beta.1)"
        if (selectedVersion === '' && loaderVersionEl && loaderVersionEl.options.length > 0) {
            const firstOption = loaderVersionEl.options[0];
            const match = (firstOption.textContent || '').match(/\(([^)]+)\)/);
            if (match) selectedVersion = match[1];
        }

        const warningText = document.getElementById('loader-warning-text');
        if (!loaderWarning) return;

        if (loader === 'quilt' && selectedVersion.startsWith('0.26.1')) {
            if (warningText) warningText.textContent = 'Quilt 0.26.1 ist experimentell und startet möglicherweise nicht zuverlässig. Für stabile Ergebnisse empfehlen wir Fabric oder eine ältere Quilt-Version.';
            loaderWarning.style.display = 'block';
        } else if (loader !== 'forge' && loader !== 'neoforge') {
            loaderWarning.style.display = 'none';
        }
    }

    // Zeige Warnung bei Forge/NeoForge Auswahl UND aktualisiere Versionen
    if (loaderSelect && loaderWarning) {
        loaderSelect.addEventListener('change', async (e) => {
            const loader = e.target.value;
            const warningText = document.getElementById('loader-warning-text');
            if (loader === 'forge' || loader === 'neoforge') {
                if (warningText) warningText.textContent = 'Forge/NeoForge support is experimental. For best results, use Fabric.';
                loaderWarning.style.display = 'block';
            } else {
                loaderWarning.style.display = 'none';
            }

            // WICHTIG: Aktualisiere die verfügbaren Versionen basierend auf dem Loader!
            await updateVersionSelects();

            // WICHTIG: Lade auch die Loader-Versionen!
            await updateCreateLoaderVersions();

            // Nach dem Laden der Versionen: Quilt 26.1 prüfen
            checkQuilt261Warning();
        });

        // Listener auf MC-Version: Loader-Versionen neu laden wenn MC-Version wechselt
        const mcVersionSelect = document.getElementById('profile-mc-version');
        if (mcVersionSelect) {
            mcVersionSelect.addEventListener('change', async () => {
                await updateCreateLoaderVersions();
                checkQuilt261Warning();
            });
        }

        // Listener auf Loader-Version: Quilt 26.1 Warnung
        const loaderVersionSelect = document.getElementById('profile-loader-version');
        if (loaderVersionSelect) {
            loaderVersionSelect.addEventListener('change', () => {
                checkQuilt261Warning();
            });
        }
    }

    if (cancelBtn) {
        cancelBtn.addEventListener('click', () => {
            debugLog('Cancel profile button clicked', 'info');
            const modal = document.getElementById('create-profile-modal');
            if (modal) modal.classList.remove('active');
        });
    }

    if (saveBtn) {
        saveBtn.addEventListener('click', createProfile);
    }

    // Alle Selects auf dark gray stylen wenn eine Option gewählt wird
    const allSelects = [
        document.getElementById('profile-mc-version'),
        document.getElementById('profile-loader'),
        document.getElementById('profile-loader-version')
    ];

    allSelects.forEach(select => {
        if (select) {
            select.addEventListener('change', (e) => {
                e.target.style.color = '#999';
                e.target.style.fontWeight = '600';
            });
        }
    });
}

let allMinecraftVersions = [];
let showSnapshots = false;
let forgeSupportedMcVersions = null; // Cache: MC-Versionen mit Forge-Support
let fabricSupportedMcVersions = null; // Cache: MC-Versionen mit Fabric-Support
let quiltSupportedMcVersions = null; // Cache: MC-Versionen mit Quilt-Support
let neoforgeSupportedMcVersions = null; // Cache: MC-Versionen mit NeoForge-Support

/// Lädt die Liste der MC-Versionen die Forge unterstützen (gecached)
async function getForgeSuportedMcVersions() {
    if (forgeSupportedMcVersions !== null) return forgeSupportedMcVersions;
    try {
        forgeSupportedMcVersions = await invoke('get_forge_supported_mc_versions');
        debugLog('Loaded ' + forgeSupportedMcVersions.length + ' Forge-supported MC versions', 'success');
    } catch (e) {
        debugLog('Failed to load Forge-supported MC versions: ' + e, 'error');
        forgeSupportedMcVersions = [];
    }
    return forgeSupportedMcVersions;
}

/// Lädt die Liste der MC-Versionen die Fabric unterstützen (gecached)
async function getFabricSupportedMcVersions() {
    if (fabricSupportedMcVersions !== null) return fabricSupportedMcVersions;
    try {
        fabricSupportedMcVersions = await invoke('get_fabric_supported_mc_versions');
        debugLog('Loaded ' + fabricSupportedMcVersions.length + ' Fabric-supported MC versions', 'success');
    } catch (e) {
        debugLog('Failed to load Fabric-supported MC versions: ' + e, 'error');
        fabricSupportedMcVersions = [];
    }
    return fabricSupportedMcVersions;
}

/// Lädt die Liste der MC-Versionen die Quilt unterstützen (gecached)
async function getQuiltSupportedMcVersions() {
    if (quiltSupportedMcVersions !== null) return quiltSupportedMcVersions;
    try {
        quiltSupportedMcVersions = await invoke('get_quilt_supported_mc_versions');
        debugLog('Loaded ' + quiltSupportedMcVersions.length + ' Quilt-supported MC versions', 'success');
    } catch (e) {
        debugLog('Failed to load Quilt-supported MC versions: ' + e, 'error');
        quiltSupportedMcVersions = [];
    }
    return quiltSupportedMcVersions;
}

/// Lädt die Liste der MC-Versionen die NeoForge unterstützen (gecached)
async function getNeoforgeSupportedMcVersions() {
    if (neoforgeSupportedMcVersions !== null) return neoforgeSupportedMcVersions;
    try {
        neoforgeSupportedMcVersions = await invoke('get_neoforge_supported_mc_versions');
        debugLog('Loaded ' + neoforgeSupportedMcVersions.length + ' NeoForge-supported MC versions', 'success');
    } catch (e) {
        debugLog('Failed to load NeoForge-supported MC versions: ' + e, 'error');
        neoforgeSupportedMcVersions = [];
    }
    return neoforgeSupportedMcVersions;
}

/// Hilfsfunktion: Filtert Versionen basierend auf dem Loader
async function filterVersionsByLoader(versions, loader) {
    if (loader === 'vanilla') return versions;

    let supported;
    if (loader === 'forge') {
        supported = await getForgeSuportedMcVersions();
    } else if (loader === 'fabric') {
        supported = await getFabricSupportedMcVersions();
    } else if (loader === 'quilt') {
        supported = await getQuiltSupportedMcVersions();
    } else if (loader === 'neoforge') {
        supported = await getNeoforgeSupportedMcVersions();
    } else {
        return versions;
    }

    if (supported && supported.length > 0) {
        const supportedSet = new Set(supported);
        const filtered = versions.filter(v => supportedSet.has(v.id));
        debugLog('Filtered to ' + filtered.length + ' ' + loader + '-supported versions', 'info');
        return filtered;
    }
    return versions;
}

async function loadMinecraftVersions() {
    try {
        debugLog('Loading Minecraft versions...', 'info');
        allMinecraftVersions = await invoke('get_minecraft_versions');
        debugLog('Loaded ' + allMinecraftVersions.length + ' versions', 'success');

        await updateVersionSelects();

        // Setup snapshot toggle
        const snapshotToggle = document.getElementById('show-snapshots');
        if (snapshotToggle) {
            snapshotToggle.addEventListener('change', (e) => {
                showSnapshots = e.target.checked;
                updateVersionSelects();
            });
        }
    } catch (error) {
        debugLog('Failed to load versions: ' + error, 'error');
    }
}

async function updateVersionSelects() {
    debugLog('Updating version selects...', 'info');

    if (allMinecraftVersions.length === 0) {
        debugLog('No versions loaded yet!', 'error');
        return;
    }

    // Filter versions based on snapshot toggle
    let filteredVersions = showSnapshots
        ? [...allMinecraftVersions]
        : allMinecraftVersions.filter(v => v.version_type?.toLowerCase() === 'release');

    // Filtere Versionen basierend auf dem gewählten Loader
    const loaderSelect = document.getElementById('profile-loader');
    const selectedLoader = loaderSelect ? loaderSelect.value : 'vanilla';

    // Für jeden Loader: Nur MC-Versionen anzeigen die tatsächlich unterstützt werden
    filteredVersions = await filterVersionsByLoader(filteredVersions, selectedLoader);

    // Update Profile Modal select
    // Vanilla hat keine Filterung → Limit auf 50 für Performance.
    // Alle anderen Loader sind bereits gefiltert → alle anzeigen.
    const versionLimit = selectedLoader === 'vanilla' ? 50 : filteredVersions.length;
    const profileSelect = document.getElementById('profile-mc-version');
    if (profileSelect) {
        debugLog('Found profile-mc-version select, updating...', 'success');
        profileSelect.innerHTML = filteredVersions.slice(0, versionLimit).map(v =>
            `<option value="${v.id}">${v.id}${v.version_type !== 'Release' ? ' (' + v.version_type + ')' : ''}</option>`
        ).join('');
    } else {
        debugLog('profile-mc-version select NOT FOUND!', 'error');
    }

    // Update Mod Browser filter (limit to 30 for dropdown)
    const filterSelect = document.getElementById('filter-version');
    if (filterSelect) {
        filterSelect.innerHTML = '<option value="">All Versions</option>' +
            filteredVersions.slice(0, 30).map(v =>
                `<option value="${v.id}">${v.id}${v.version_type !== 'Release' ? ' (' + v.version_type + ')' : ''}</option>`
            ).join('');
    }
}

// Lade Loader-Versionen für das Create-Profile-Modal
async function updateCreateLoaderVersions() {
    const loaderSelect = document.getElementById('profile-loader');
    const versionSelect = document.getElementById('profile-loader-version');
    const mcVersionSelect = document.getElementById('profile-mc-version');

    if (!loaderSelect || !versionSelect || !mcVersionSelect) return;

    const loader = loaderSelect.value;
    const mcVersion = mcVersionSelect.value;

    if (!mcVersion) {
        versionSelect.innerHTML = '<option value="">Wähle zuerst MC-Version</option>';
        versionSelect.disabled = true;
        return;
    }

    if (loader === 'vanilla') {
        versionSelect.innerHTML = '<option value="">-</option>';
        versionSelect.disabled = true;
        return;
    }

    versionSelect.disabled = false;
    versionSelect.innerHTML = '<option value="">Lade...</option>';

    try {
        let versions = [];

        if (loader === 'fabric') {
            versions = await invoke('get_fabric_versions', { minecraftVersion: mcVersion });
        } else if (loader === 'quilt') {
            versions = await invoke('get_quilt_versions', { minecraftVersion: mcVersion });
        } else if (loader === 'forge') {
            versions = await invoke('get_forge_versions', { minecraftVersion: mcVersion });
        } else if (loader === 'neoforge') {
            versions = await invoke('get_neoforge_versions', { minecraftVersion: mcVersion });
        }

        if (versions && versions.length > 0) {
            // Zeige die neueste als Standard + alle anderen
            versionSelect.innerHTML = '<option value="">Neueste (' + versions[0] + ')</option>' +
                versions.map(v => `<option value="${v}">${v}</option>`).join('');
        } else {
            versionSelect.innerHTML = '<option value="">Neueste</option>';
        }
    } catch (error) {
        console.error('Failed to load loader versions:', error);
        if (loader === 'quilt') {
            // Quilt unterstützt diese MC-Version noch nicht direkt – der Launcher nutzt
            // beim Start automatisch einen kompatiblen Fallback-Loader.
            versionSelect.innerHTML = '<option value="">Neueste (kompatibler Loader)</option>';
            showToast('Quilt hat für diese MC-Version noch keine offizielle Unterstützung – der Launcher nutzt automatisch den kompatiblen Fallback-Loader.', 'info', 6000);
        } else {
            versionSelect.innerHTML = '<option value="">Neueste (Fehler beim Laden)</option>';
        }
    }
}

async function createProfile() {
    const nameInput = document.getElementById('profile-name');
    const versionInput = document.getElementById('profile-mc-version');
    const loaderInput = document.getElementById('profile-loader');
    const loaderVersionInput = document.getElementById('profile-loader-version');

    if (!nameInput || !versionInput || !loaderInput || !loaderVersionInput) {
        debugLog('Form elements not found', 'error');
        return;
    }

    const name = nameInput.value;
    const mcVersion = versionInput.value;
    const loader = loaderInput.value;
    const loaderVersion = loaderVersionInput.value;

    if (!name || !mcVersion) {
        showToast('Please fill in all required fields', 'warning', 3500);
        return;
    }

    try {
        debugLog('Creating profile: ' + name, 'info');
        const knownProfileIds = new Set(profiles.map(p => p.id));
        const profileList = await invoke('create_profile', {
            name,
            minecraftVersion: mcVersion,
            loader,
            loaderVersion
        });

        profiles = profileList.profiles || [];
        renderProfiles();

        const createdProfile = profiles.find(p => !knownProfileIds.has(p.id)) ||
            profiles.find(p => p.name === name && p.minecraft_version === mcVersion) ||
            null;

        setSessionWidgetState({
            profileId: createdProfile ? createdProfile.id : null,
            profileName: createdProfile ? createdProfile.name : name,
            phase: 'queued',
            status: 'Profil erstellt - starte Download...',
            percent: null,
            canStop: false
        });
        animateSessionFlyIn(createdProfile ? createdProfile.name : name);

        const modal = document.getElementById('create-profile-modal');
        if (modal) modal.classList.remove('active');
        nameInput.value = '';

        if (createdProfile && createdProfile.id) {
            try {
                await prepareProfileDownloadFlow(createdProfile.id, createdProfile.name);
            } catch (prepareError) {
                showToast('Profil erstellt, aber Download fehlgeschlagen: ' + prepareError, 'error', 5000);
            }
        }

        debugLog('Profile created successfully', 'success');
    } catch (error) {
        debugLog('Failed to create profile: ' + error, 'error');
        showToast('Failed to create profile: ' + error, 'error', 5000);
    }
}

// Mod Browser
let installedModIds = new Set(); // Cache für installierte Mod-IDs
let installedModsCache = []; // Cache der geladenen installierten Mods (für Suchfilter)
let cachedModsProfileId = null; // Profil-ID zu der der Cache gehört
let installedResourcePackNames = new Set(); // Cache für installierte Resource Packs
let installedShaderPackNames = new Set(); // Cache für installierte Shader Packs
let installedModIdsProfileId = null;
let installedResourcePackNamesProfileId = null;
let installedShaderPackNamesProfileId = null;

const profileModsCache = new Map();
const profileResourcePacksCache = new Map();
const profileShaderPacksCache = new Map();
const browserResultsCache = new Map();
const modrinthCategoriesCache = new Map();
const BROWSER_RESULTS_CACHE_TTL_MS = 2 * 60 * 1000;

function normalizePackKey(value) {
    if (value == null) return '';
    return String(value)
        .toLowerCase()
        .trim()
        .replace(/\.(zip|jar)$/i, '')
        .replace(/[_\s]+/g, '-')
        .replace(/[^a-z0-9.-]+/g, '-')
        .replace(/-+/g, '-')
        .replace(/^-|-$/g, '');
}

function addInstalledPackKeys(setRef, pack) {
    const add = (value) => {
        const key = normalizePackKey(value);
        if (key) setRef.add(key);
    };

    add(pack.name);
    add(pack.project_id);
    add(pack.slug);
    add(pack.title);

    const noExt = String(pack.name || '').replace(/\.(zip|jar)$/i, '');
    add(noExt);
    const firstPart = noExt.split('-')[0];
    if (firstPart && firstPart.length > 2) add(firstPart);
}

function buildBrowserCacheKey(contentType, query, page) {
    return JSON.stringify({
        contentType,
        query: (query || '').trim().toLowerCase(),
        page,
        version: selectedFilters.version || '',
        loader: selectedFilters.loader || '',
        sort: selectedFilters.sort || '',
        categories: [...(selectedFilters.categories || [])].sort(),
        hideInstalled: !!selectedFilters.hideInstalled,
        effectiveLimit: getEffectiveLimit(),
        profileId: currentProfile ? currentProfile.id : null
    });
}

function getCachedBrowserResults(cacheKey) {
    const entry = browserResultsCache.get(cacheKey);
    if (!entry) return null;
    if (Date.now() - entry.ts > BROWSER_RESULTS_CACHE_TTL_MS) {
        browserResultsCache.delete(cacheKey);
        return null;
    }
    return Array.isArray(entry.items) ? entry.items : null;
}

function setCachedBrowserResults(cacheKey, items) {
    browserResultsCache.set(cacheKey, {
        ts: Date.now(),
        items: Array.isArray(items) ? items : []
    });
}

function invalidateProfileContentCaches(profileId) {
    if (!profileId) return;
    profileModsCache.delete(profileId);
    profileResourcePacksCache.delete(profileId);
    profileShaderPacksCache.delete(profileId);

    if (cachedModsProfileId === profileId) {
        cachedModsProfileId = null;
        installedModsCache = [];
    }
    if (installedModIdsProfileId === profileId) {
        installedModIdsProfileId = null;
        installedModIds.clear();
    }
    if (installedResourcePackNamesProfileId === profileId) {
        installedResourcePackNamesProfileId = null;
        installedResourcePackNames.clear();
    }
    if (installedShaderPackNamesProfileId === profileId) {
        installedShaderPackNamesProfileId = null;
        installedShaderPackNames.clear();
    }
}

function resolvePackIconSrc(pack) {
    if (!pack) return '';
    if (pack.icon_path) {
        if (typeof pack.icon_path === 'string' && pack.icon_path.startsWith('data:image/')) {
            return pack.icon_path;
        }
        return `file://${encodeURI(pack.icon_path)}`;
    }
    const iconUrl = normalizeMediaUrl(pack.icon_url);
    return iconUrl || '';
}

function setupSearch() {
    const searchInput = document.getElementById('mod-search');
    if (!searchInput) return;

    let searchTimeout;
    searchInput.addEventListener('input', (e) => {
        clearTimeout(searchTimeout);
        searchTimeout = setTimeout(() => {
            const query = e.target.value;
            if (currentContentType === 'mods') {
                searchMods(query);
            } else if (currentContentType === 'resourcepacks') {
                searchResourcePacks(query);
            } else if (currentContentType === 'shaderpacks') {
                searchShaderPacks(query);
            } else if (currentContentType === 'modpacks') {
                searchModpacks(query);
            }
        }, 500);
    });

    // Sort By Dropdown
    const sortFilter = document.getElementById('filter-sort');
    if (sortFilter) {
        sortFilter.addEventListener('change', (e) => {
            selectedFilters.sort = e.target.value;
            const query = searchInput.value;
            if (currentContentType === 'mods') {
                searchMods(query);
            } else if (currentContentType === 'resourcepacks') {
                searchResourcePacks(query);
            } else if (currentContentType === 'shaderpacks') {
                searchShaderPacks(query);
            } else if (currentContentType === 'modpacks') {
                searchModpacks(query);
            }
        });
    }

    // Mod Loader Dropdown
    const loaderFilter = document.getElementById('filter-loader');
    if (loaderFilter) {
        loaderFilter.addEventListener('change', (e) => {
            selectedFilters.loader = e.target.value;
            searchMods(searchInput.value);
        });
    }

    // Version Filter
    const versionFilter = document.getElementById('filter-version');
    if (versionFilter) {
        versionFilter.addEventListener('change', (e) => {
            selectedFilters.version = e.target.value;
            const query = searchInput.value;
            if (currentContentType === 'mods') {
                searchMods(query);
            } else if (currentContentType === 'resourcepacks') {
                searchResourcePacks(query);
            } else if (currentContentType === 'shaderpacks') {
                searchShaderPacks(query);
            } else if (currentContentType === 'modpacks') {
                searchModpacks(query);
            }
        });
    }

    // Hide Installed Filter
    const hideInstalledFilter = document.getElementById('filter-hide-installed');
    if (hideInstalledFilter) {
        hideInstalledFilter.addEventListener('change', (e) => {
            selectedFilters.hideInstalled = e.target.checked;
            currentModPage = 0;
            // Alte Page-Caches verwerfen, damit Overfetch + Pagination konsistent bleibt.
            browserResultsCache.clear();
            triggerContentSearch();
        });
    }

    // Lade Kategorien von der Modrinth API
    loadModrinthCategories();

    // Lade automatisch beliebte Inhalte beim Start
    loadPopularContent();
}

// Content Type Switching
function switchContentType(type, options = {}) {
    debugLog('switchContentType called with type: ' + type, 'warn');
    debugLog('currentContentType BEFORE: ' + currentContentType, 'warn');

    const forceReload = !!options.force;
    if (!forceReload && currentContentType === type) {
        return;
    }

    currentContentType = type;
    patchCurrentNavLocation(e => e.type === 'browser', { contentType: type });

    debugLog('currentContentType AFTER: ' + currentContentType, 'warn');

    currentModPage = 0;
    currentModSearchQuery = '';

    // Reset categories beim Wechsel
    selectedFilters.categories = [];

    // Update Button States
    document.querySelectorAll('[data-content-type]').forEach(btn => {
        if (btn.dataset.contentType === type) {
            btn.classList.add('active');
        } else {
            btn.classList.remove('active');
        }
    });

    // Show/Hide Mod Loader Filter (nur für Mods und Modpacks relevant)
    const loaderSection = document.getElementById('loader-filter-section');
    if (loaderSection) {
        loaderSection.style.display = (type === 'mods' || type === 'modpacks') ? 'block' : 'none';
    }

    // Update Placeholder
    const searchInput = document.getElementById('mod-search');
    if (searchInput) {
        const placeholders = {
            mods: 'Search mods...',
            resourcepacks: 'Search resource packs...',
            shaderpacks: 'Search shader packs...',
            modpacks: 'Search modpacks...'
        };
        searchInput.placeholder = serverBrowserContext?.projectType === 'plugin'
            ? 'Search plugins...'
            : (placeholders[type] || 'Search content...');
        searchInput.value = '';
    }

    // Lade Kategorien für den neuen Content-Typ
    loadModrinthCategories();

    // Lade Inhalte
    loadPopularContent();
}

function loadPopularContent() {
    debugLog('loadPopularContent called with currentContentType: ' + currentContentType, 'warn');

    if (currentContentType === 'mods') {
        debugLog('Loading MODS', 'warn');
        loadPopularMods();
    } else if (currentContentType === 'resourcepacks') {
        debugLog('Loading RESOURCEPACKS', 'warn');
        loadPopularResourcePacks();
    } else if (currentContentType === 'shaderpacks') {
        debugLog('Loading SHADERPACKS', 'warn');
        loadPopularShaderPacks();
    } else if (currentContentType === 'modpacks') {
        debugLog('Loading MODPACKS', 'warn');
        loadPopularModpacks();
    } else {
        debugLog('UNKNOWN currentContentType: ' + currentContentType, 'error');
    }
}

// Lade Kategorien von der Modrinth API (gruppiert nach header wie im Modrinth Launcher)
async function loadModrinthCategories() {
    const categoriesContainer = document.getElementById('filter-categories');
    if (!categoriesContainer) return;

    categoriesContainer.innerHTML = '<div style="color: var(--text-secondary); font-size: 12px; padding: 10px;">Loading categories...</div>';

    try {
        // Lade Kategorien über Backend (kein CORS-Problem!) + Cache
        let allCategories = modrinthCategoriesCache.get('all');
        if (!allCategories) {
            allCategories = await invoke('get_modrinth_categories');
            modrinthCategoriesCache.set('all', allCategories || []);
        }

        // Filtern nach Content-Type
        const projectType = currentContentType === 'mods' ? 'mod' :
            currentContentType === 'modpacks' ? 'modpack' :
                currentContentType === 'resourcepacks' ? 'resourcepack' :
                    currentContentType === 'shaderpacks' ? 'shader' : 'mod';

        const categories = allCategories.filter(cat => cat.project_type === projectType);

        if (categories && categories.length > 0) {
            categoriesContainer.innerHTML = '';

            // Gruppiere nach header
            const grouped = {};
            categories.forEach(cat => {
                const header = cat.header || 'other';
                if (!grouped[header]) {
                    grouped[header] = [];
                }
                grouped[header].push(cat);
            });

            // Header-Reihenfolge definieren
            const headerOrder = ['categories', 'features', 'resolutions', 'performance impact', 'other'];
            const headerLabels = {
                'categories': 'Categories',
                'features': 'Features',
                'resolutions': 'Resolutions',
                'performance impact': 'Performance',
                'other': 'Other'
            };

            // Für jeden Header eine Gruppe erstellen
            headerOrder.forEach(headerKey => {
                if (!grouped[headerKey] || grouped[headerKey].length === 0) return;

                // Header-Titel
                const headerDiv = document.createElement('div');
                headerDiv.style.cssText = `
                    font-size: 11px;
                    font-weight: 600;
                    color: var(--text-secondary);
                    text-transform: uppercase;
                    letter-spacing: 0.5px;
                    padding: 12px 8px 6px 8px;
                    margin-top: 8px;
                    border-top: 1px solid var(--bg-light);
                `;
                if (headerKey === headerOrder[0] || !grouped[headerOrder[0]]) {
                    headerDiv.style.borderTop = 'none';
                    headerDiv.style.marginTop = '0';
                }
                headerDiv.textContent = headerLabels[headerKey] || headerKey;
                categoriesContainer.appendChild(headerDiv);

                // Kategorien in dieser Gruppe
                grouped[headerKey].forEach(cat => {
                    const label = document.createElement('label');
                    label.style.cssText = `
                        display: flex;
                        align-items: center;
                        gap: 8px;
                        padding: 6px 8px;
                        cursor: pointer;
                        border-radius: 6px;
                        transition: background 0.2s;
                        font-size: 13px;
                    `;

                    label.addEventListener('mouseenter', () => {
                        label.style.background = 'var(--bg-light)';
                    });
                    label.addEventListener('mouseleave', () => {
                        label.style.background = 'transparent';
                    });

                    const checkbox = document.createElement('input');
                    checkbox.type = 'checkbox';
                    checkbox.value = cat.name;
                    checkbox.style.cssText = `
                        cursor: pointer;
                        width: 16px;
                        height: 16px;
                        accent-color: var(--gold);
                    `;
                    checkbox.addEventListener('change', (e) => {
                        if (e.target.checked) {
                            selectedFilters.categories.push(cat.name);
                        } else {
                            selectedFilters.categories = selectedFilters.categories.filter(c => c !== cat.name);
                        }
                        // Trigger search mit aktuellen Filtern
                        triggerContentSearch();
                    });

                    // Icon (wenn vorhanden, als SVG)
                    if (cat.icon && cat.icon.startsWith('<svg')) {
                        const iconSpan = document.createElement('span');
                        iconSpan.innerHTML = cat.icon;
                        iconSpan.style.cssText = `
                            width: 16px;
                            height: 16px;
                            display: flex;
                            align-items: center;
                            justify-content: center;
                            color: var(--text-secondary);
                        `;
                        const svg = iconSpan.querySelector('svg');
                        if (svg) {
                            svg.style.width = '14px';
                            svg.style.height = '14px';
                        }
                        label.appendChild(checkbox);
                        label.appendChild(iconSpan);
                    } else {
                        label.appendChild(checkbox);
                    }

                    const text = document.createElement('span');
                    // Formatiere den Namen schöner
                    text.textContent = cat.name
                        .split('-')
                        .map(word => word.charAt(0).toUpperCase() + word.slice(1))
                        .join(' ');
                    text.style.cssText = `
                        color: var(--text-primary);
                        flex: 1;
                    `;

                    label.appendChild(text);
                    categoriesContainer.appendChild(label);
                });
            });
        } else {
            categoriesContainer.innerHTML = '<div style="color: var(--text-secondary); font-size: 12px; padding: 10px;">No categories available</div>';
        }
    } catch (error) {
        debugLog('Failed to load categories: ' + error, 'error');
        categoriesContainer.innerHTML = '<div style="color: var(--text-secondary); font-size: 12px; padding: 10px;">Failed to load categories</div>';
    }
}

// Helper-Funktion um Suche mit aktuellen Filtern zu triggern
function triggerContentSearch() {
    const searchInput = document.getElementById('mod-search');
    const query = searchInput ? searchInput.value : '';
    if (currentContentType === 'mods') {
        searchMods(query);
    } else if (currentContentType === 'resourcepacks') {
        searchResourcePacks(query);
    } else if (currentContentType === 'shaderpacks') {
        searchShaderPacks(query);
    } else if (currentContentType === 'modpacks') {
        searchModpacks(query);
    }
}

// Lade Environment-Icons von der Modrinth API
async function loadEnvironmentIcons() {
    try {
        debugLog('Loading environment icons...', 'info');

        // Lade alle Kategorien von Modrinth über Backend
        const categories = await invoke('get_modrinth_categories');

        // Finde die Environment-Kategorien für Mods
        const envCategories = {
            'client': categories.find(cat => cat.project_type === 'mod' && (cat.name === 'client-side' || cat.name === 'client')),
            'server': categories.find(cat => cat.project_type === 'mod' && (cat.name === 'server-side' || cat.name === 'server')),
            'both': categories.find(cat => cat.project_type === 'mod' && (cat.name === 'client-and-server' || cat.name === 'clientandserver')),
            'or': categories.find(cat => cat.project_type === 'mod' && (cat.name === 'client-or-server' || cat.name === 'clientorserver'))
        };

        // Füge Icons zu jedem Label hinzu
        Object.entries(envCategories).forEach(([type, cat]) => {
            if (!cat || !cat.icon) return;

            const label = document.getElementById(`env-${type}-label`);
            if (!label) return;

            const checkbox = label.querySelector('input');
            const textSpan = label.querySelector('span');

            if (!checkbox || !textSpan) return;

            // Erstelle Icon-Span (genau wie bei Categories)
            const iconSpan = document.createElement('span');
            iconSpan.innerHTML = cat.icon;
            iconSpan.style.cssText = `
                width: 16px;
                height: 16px;
                display: flex;
                align-items: center;
                justify-content: center;
                color: var(--text-secondary);
                flex-shrink: 0;
            `;

            const svg = iconSpan.querySelector('svg');
            if (svg) {
                svg.style.width = '14px';
                svg.style.height = '14px';
            }

            // Füge Icon zwischen Checkbox und Text ein
            textSpan.parentNode.insertBefore(iconSpan, textSpan);
            debugLog(`Environment icon added for ${type}: ${cat.name}`, 'success');
        });

        debugLog('Environment icons loaded successfully', 'success');
    } catch (error) {
        debugLog('Failed to load environment icons: ' + error, 'error');
    }
}

// Lädt beliebte Resource Packs
async function loadPopularResourcePacks(page = 0) {
    const modList = document.getElementById('mod-list');
    if (!modList) return;

    currentModPage = page;
    currentModSearchQuery = '';

    modList.innerHTML = '<div class="loading"><div class="spinner" style="margin: 20px auto;"></div><p>Lade beliebte Resource Packs...</p></div>';

    // Lade installierte Resource Packs für Markierung
    await loadInstalledResourcePackNames();

    // Lade auch installierte Mods für den Fall dass User zu Mods wechselt
    await loadInstalledModIds();

    const cacheKey = buildBrowserCacheKey('resourcepacks', '', page);
    const cached = getCachedBrowserResults(cacheKey);
    if (cached) {
        renderMods(cached, page);
        return;
    }

    try {
        const packs = await invoke('search_resourcepacks', {
            query: '',
            gameVersion: selectedFilters.version || null,
            categories: selectedFilters.categories.length > 0 ? selectedFilters.categories : null,
            sortBy: 'downloads',
            offset: page * getEffectiveLimit(),
            limit: getEffectiveLimit()
        });

        setCachedBrowserResults(cacheKey, packs);
        renderMods(packs, page);
    } catch (error) {
        debugLog('Failed to load resource packs: ' + error, 'error');
        modList.innerHTML = `
            <div class="loading" style="text-align: center; padding: 40px;">
                <div style="font-size: 48px; margin-bottom: 15px;"><i class="bi bi-palette"></i></div>
                <p style="color: var(--gold); margin-bottom: 10px;">Beliebte Resource Packs</p>
                <p style="color: var(--text-secondary);">Gib einen Suchbegriff ein oder versuche es später erneut</p>
            </div>
        `;
    }
}

async function searchResourcePacks(query, page = 0) {
    const modList = document.getElementById('mod-list');
    if (!modList) return;

    currentModSearchQuery = query || '';
    currentModPage = page;

    if (!query || query.length < 2) {
        loadPopularResourcePacks(page);
        return;
    }

    modList.innerHTML = '<div class="loading"><div class="spinner" style="margin: 20px auto;"></div><p>Suche...</p></div>';

    // Lade installierte Resource Packs für Markierung
    await loadInstalledResourcePackNames();

    const cacheKey = buildBrowserCacheKey('resourcepacks', query, page);
    const cached = getCachedBrowserResults(cacheKey);
    if (cached) {
        renderMods(cached, page);
        return;
    }

    try {
        const packs = await invoke('search_resourcepacks', {
            query,
            gameVersion: selectedFilters.version || null,
            categories: selectedFilters.categories.length > 0 ? selectedFilters.categories : null,
            sortBy: selectedFilters.sort || 'downloads',
            offset: page * getEffectiveLimit(),
            limit: getEffectiveLimit()
        });

        setCachedBrowserResults(cacheKey, packs);
        renderMods(packs, page);
    } catch (error) {
        debugLog('Search failed: ' + error, 'error');
        modList.innerHTML = '<div class="loading">Suche fehlgeschlagen: ' + error + '</div>';
    }
}

async function installResourcePack(packId) {
    let profile = currentProfile;

    if (!profile) {
        profile = await showProfileSelectDialog();
        if (!profile) return;
    }

    debugLog('Installing resource pack ' + packId + ' to profile ' + profile.name, 'info');

    // Markiere Button als "installierend"
    const btn = event.target;
    const originalText = btn.textContent;
    btn.textContent = '...';
    btn.disabled = true;

    try {
        await invoke('install_resourcepack', {
            profileId: profile.id,
            packId: packId,
            versionId: null
        });

        debugLog('Resource pack installed successfully!', 'success');
        showToast(`Resource Pack erfolgreich zu "${profile.name}" hinzugefügt!`, 'success', 3000);

        // Button als installiert markieren mit korrektem Styling
        btn.innerHTML = '<i class="bi bi-check"></i> Installiert';
        btn.disabled = true;
        btn.style.background = 'var(--bg-light)';
        btn.style.color = 'var(--text-secondary)';
        btn.style.opacity = '0.7';
        btn.style.cursor = 'not-allowed';

        // Cache aktualisieren
        invalidateProfileContentCaches(profile.id);
        await loadInstalledResourcePackNames(true);

    } catch (error) {
        debugLog('Install failed: ' + error, 'error');
        showToast('Resource Pack-Installation fehlgeschlagen: ' + error, 'error', 5000);
        btn.textContent = originalText;
        btn.disabled = false;
    }
}

async function installShaderPack(packId) {
    let profile = currentProfile;

    if (!profile) {
        profile = await showProfileSelectDialog();
        if (!profile) return;
    }

    debugLog('Installing shader pack ' + packId + ' to profile ' + profile.name, 'info');

    // Markiere Button als "installierend"
    const btn = event.target;
    const originalText = btn.textContent;
    btn.textContent = '...';
    btn.disabled = true;

    try {
        await invoke('install_shaderpack', {
            profileId: profile.id,
            packId: packId,
            versionId: null
        });

        debugLog('Shader pack installed successfully!', 'success');
        showToast(`Shader Pack erfolgreich zu "${profile.name}" hinzugefügt!`, 'success', 3000);

        // Button als installiert markieren mit korrektem Styling
        btn.innerHTML = '<i class="bi bi-check"></i> Installiert';
        btn.disabled = true;
        btn.style.background = 'var(--bg-light)';
        btn.style.color = 'var(--text-secondary)';
        btn.style.opacity = '0.7';
        btn.style.cursor = 'not-allowed';

        // Cache aktualisieren
        invalidateProfileContentCaches(profile.id);
        await loadInstalledShaderPackNames(true);

    } catch (error) {
        debugLog('Install failed: ' + error, 'error');
        showToast('Shader Pack-Installation fehlgeschlagen: ' + error, 'error', 5000);
        btn.textContent = originalText;
        btn.disabled = false;
    }
}

// ==================== SHADER PACKS ====================

async function loadPopularShaderPacks(page = 0) {
    const modList = document.getElementById('mod-list');
    if (!modList) return;

    currentModPage = page;
    currentModSearchQuery = '';

    modList.innerHTML = '<div class="loading"><div class="spinner" style="margin: 20px auto;"></div><p>Lade beliebte Shader Packs...</p></div>';

    // Lade installierte Shader für Markierung
    await loadInstalledShaderPackNames();

    // Lade auch installierte Mods für den Fall dass User zu Mods wechselt
    await loadInstalledModIds();

    const cacheKey = buildBrowserCacheKey('shaderpacks', '', page);
    const cached = getCachedBrowserResults(cacheKey);
    if (cached) {
        renderMods(cached, page);
        return;
    }

    try {
        const packs = await invoke('search_shaderpacks', {
            query: '',
            gameVersion: selectedFilters.version || null,
            categories: selectedFilters.categories.length > 0 ? selectedFilters.categories : null,
            sortBy: 'downloads',
            offset: page * getEffectiveLimit(),
            limit: getEffectiveLimit()
        });

        setCachedBrowserResults(cacheKey, packs);
        renderMods(packs, page);
    } catch (error) {
        debugLog('Failed to load shader packs: ' + error, 'error');
        modList.innerHTML = `
            <div class="loading" style="text-align: center; padding: 40px;">
                <div style="font-size: 48px; margin-bottom: 15px;"><i class="bi bi-stars"></i></div>
                <p style="color: var(--gold); margin-bottom: 10px;">Beliebte Shader Packs</p>
                <p style="color: var(--text-secondary);">Gib einen Suchbegriff ein oder versuche es später erneut</p>
            </div>
        `;
    }
}

async function searchShaderPacks(query, page = 0) {
    const modList = document.getElementById('mod-list');
    if (!modList) return;

    currentModSearchQuery = query || '';
    currentModPage = page;

    if (!query || query.length < 2) {
        loadPopularShaderPacks(page);
        return;
    }

    modList.innerHTML = '<div class="loading"><div class="spinner" style="margin: 20px auto;"></div><p>Suche...</p></div>';

    // Lade installierte Shader für Markierung
    await loadInstalledShaderPackNames();

    const cacheKey = buildBrowserCacheKey('shaderpacks', query, page);
    const cached = getCachedBrowserResults(cacheKey);
    if (cached) {
        renderMods(cached, page);
        return;
    }

    try {
        const packs = await invoke('search_shaderpacks', {
            query,
            gameVersion: selectedFilters.version || null,
            categories: selectedFilters.categories.length > 0 ? selectedFilters.categories : null,
            sortBy: selectedFilters.sort || 'downloads',
            offset: page * getEffectiveLimit(),
            limit: getEffectiveLimit()
        });

        setCachedBrowserResults(cacheKey, packs);
        renderMods(packs, page);
    } catch (error) {
        debugLog('Search failed: ' + error, 'error');
        modList.innerHTML = '<div class="loading">Suche fehlgeschlagen: ' + error + '</div>';
    }
}

async function installModpack(packId) {
    // Suche den Modpack-Namen aus der Mod-Card im DOM
    let packName = packId;
    const card = document.querySelector(`[data-mod-id="${packId}"]`);
    if (card) {
        const nameEl = card.querySelector('.mod-name');
        if (nameEl) packName = nameEl.textContent.trim();
    }

    // Zeige Installations-Dialog
    const modalId = 'modpack-install-modal';
    const existingModal = document.getElementById(modalId);
    if (existingModal) existingModal.remove();

    const modal = document.createElement('div');
    modal.id = modalId;
    modal.style.cssText = `
        position: fixed; top: 0; left: 0; right: 0; bottom: 0;
        background: rgba(0,0,0,0.85); display: flex; align-items: center;
        justify-content: center; z-index: 10000;
    `;
    modal.innerHTML = `
        <div style="background: var(--bg-dark); border: 2px solid var(--gold); border-radius: 12px;
                    padding: 40px; text-align: center; min-width: 420px; max-width: 520px;">
            <div style="font-size: 48px; margin-bottom: 20px;"><i class="bi bi-collection"></i></div>
            <h2 style="color: var(--gold); margin: 0 0 10px 0;">Modpack installieren</h2>
            <p style="color: var(--text-secondary); margin-bottom: 25px;" id="mp-pack-name">${escapeHtml(packName)}</p>

            <div id="mp-status" style="color: var(--text-secondary); margin-bottom: 20px; font-size: 14px;">
                Vorbereitung...
            </div>

            <div style="background: var(--bg-light); border-radius: 8px; height: 8px; overflow: hidden; margin-bottom: 25px;">
                <div id="mp-progress-bar" style="background: var(--gold); height: 100%; width: 0;
                     transition: width 0.4s ease; border-radius: 8px;"></div>
            </div>

            <div id="mp-result" style="display: none;">
                <div id="mp-result-icon" style="font-size: 40px; margin-bottom: 12px;"></div>
                <p id="mp-result-text" style="margin-bottom: 20px; font-size: 14px;"></p>
                <button class="btn" onclick="document.getElementById('${modalId}').remove(); loadProfiles();"
                        style="padding: 10px 30px;">
                    OK
                </button>
            </div>
        </div>
    `;
    document.body.appendChild(modal);

    const setStatus = (text, pct) => {
        const el = document.getElementById('mp-status');
        const bar = document.getElementById('mp-progress-bar');
        if (el) el.textContent = text;
        if (bar) bar.style.width = pct + '%';
    };

    const showResult = (success, message) => {
        const statusDiv = document.getElementById('mp-status');
        const bar = document.getElementById('mp-progress-bar');
        const result = document.getElementById('mp-result');
        const icon = document.getElementById('mp-result-icon');
        const text = document.getElementById('mp-result-text');
        if (statusDiv) statusDiv.style.display = 'none';
        if (bar) { bar.style.width = '100%'; bar.style.background = success ? '#4caf50' : '#f44336'; }
        if (result) result.style.display = 'block';
        if (icon) icon.innerHTML = success
            ? '<i class="bi bi-check-circle-fill" style="color:#4caf50;"></i>'
            : '<i class="bi bi-x-circle-fill" style="color:#f44336;"></i>';
        if (text) { text.textContent = message; text.style.color = success ? '#4caf50' : '#f44336'; }
    };

    try {
        setStatus('🖼️ Lade Modpack-Icon & Versions-Info...', 10);
        await new Promise(r => setTimeout(r, 100));

        setStatus('📥 Lade .mrpack herunter...', 25);

        const result = await invoke('install_modpack', {
            packId,
            packName,
            versionId: null
        });

        setStatus('✅ Abgeschlossen!', 100);
        await new Promise(r => setTimeout(r, 300));

        const modsCount = result.mods_downloaded || 0;
        const overrides = result.overrides_copied || 0;
        const hasIcon = result.has_icon ? ' · Icon übernommen ✓' : '';
        showResult(true,
            `"${escapeHtml(result.profile_name)}" erfolgreich installiert!\n\n` +
            `🎮 Minecraft ${result.minecraft_version}\n` +
            `📦 ${modsCount} Mods heruntergeladen\n` +
            `📁 ${overrides} Config/Override-Dateien kopiert${hasIcon}\n\n` +
            `Das Profil wurde automatisch erstellt.`
        );

        showToast(`Modpack "${escapeHtml(packName)}" installiert!`, 'success', 4000);

    } catch (error) {
        debugLog('Modpack install failed: ' + error, 'error');
        showResult(false, 'Installation fehlgeschlagen:\n' + String(error));
        showToast('Modpack-Installation fehlgeschlagen: ' + error, 'error', 5000);
    }
}

// ==================== MODPACKS ====================

async function loadPopularModpacks(page = 0) {
    const modList = document.getElementById('mod-list');
    if (!modList) return;

    currentModPage = page;
    currentModSearchQuery = '';

    modList.innerHTML = '<div class="loading"><div class="spinner" style="margin: 20px auto;"></div><p>Lade beliebte Modpacks...</p></div>';

    try {
        const packs = await invoke('search_modpacks', {
            query: '',
            gameVersion: selectedFilters.version || null,
            loader: selectedFilters.loader || null,
            categories: selectedFilters.categories.length > 0 ? selectedFilters.categories : null,
            sortBy: 'downloads',
            offset: page * getEffectiveLimit(),
            limit: getEffectiveLimit()
        });

        renderMods(packs, page);
    } catch (error) {
        debugLog('Failed to load modpacks: ' + error, 'error');
        modList.innerHTML = `
            <div class="loading" style="text-align: center; padding: 40px;">
                <div style="font-size: 48px; margin-bottom: 15px;"><i class="bi bi-collection"></i></div>
                <p style="color: var(--gold); margin-bottom: 10px;">Beliebte Modpacks</p>
                <p style="color: var(--text-secondary);">Gib einen Suchbegriff ein oder versuche es später erneut</p>
                <p style="color: #f44336; font-size: 12px; margin-top: 10px;">${escapeHtml(String(error))}</p>
            </div>
        `;
    }
}

async function searchModpacks(query, page = 0) {
    const modList = document.getElementById('mod-list');
    if (!modList) return;

    currentModSearchQuery = query || '';
    currentModPage = page;

    if (!query || query.length < 2) {
        loadPopularModpacks(page);
        return;
    }

    modList.innerHTML = '<div class="loading"><div class="spinner" style="margin: 20px auto;"></div><p>Suche Modpacks...</p></div>';

    try {
        const packs = await invoke('search_modpacks', {
            query,
            gameVersion: selectedFilters.version || null,
            loader: selectedFilters.loader || null,
            categories: selectedFilters.categories.length > 0 ? selectedFilters.categories : null,
            sortBy: selectedFilters.sort || 'downloads',
            offset: page * getEffectiveLimit(),
            limit: getEffectiveLimit()
        });

        renderMods(packs, page);
    } catch (error) {
        debugLog('Modpack search failed: ' + error, 'error');
        modList.innerHTML = '<div class="loading">Suche fehlgeschlagen: ' + escapeHtml(String(error)) + '</div>';
    }
}

// Lädt beliebte Mods (ohne Suchbegriff, sortiert nach Downloads)
async function loadPopularMods(page = 0) {
    if (serverBrowserContext) {
        return searchServerContent('', page);
    }
    const modList = document.getElementById('mod-list');
    if (!modList) return;

    currentModPage = page;
    currentModSearchQuery = '';

    modList.innerHTML = '<div class="loading"><div class="spinner" style="margin: 20px auto;"></div><p>Lade beliebte Mods...</p></div>';

    // Zuerst installierte Mods laden um sie zu markieren
    await loadInstalledModIds();

    const cacheKey = buildBrowserCacheKey('mods', '', page);
    const cached = getCachedBrowserResults(cacheKey);
    if (cached) {
        renderMods(cached, page);
        return;
    }

    try {
        // Suche nach beliebten Mods (leerer Query = alle, sortiert nach Downloads)
        const mods = await invoke('search_mods', {
            query: '',  // Leer für alle
            gameVersion: selectedFilters.version || null,
            loader: selectedFilters.loader || null,
            categories: selectedFilters.categories.length > 0 ? selectedFilters.categories : null,
            sortBy: 'downloads',  // Nach Downloads sortieren
            offset: page * getEffectiveLimit(),
            limit: getEffectiveLimit()
        });

        setCachedBrowserResults(cacheKey, mods);
        renderMods(mods, page);
    } catch (error) {
        debugLog('Failed to load popular mods: ' + error, 'error');
        modList.innerHTML = `
            <div class="loading" style="text-align: center; padding: 40px;">
                <div style="font-size: 48px; margin-bottom: 15px;"><i class="bi bi-collection-fill"></i></div>
                <p style="color: var(--gold); margin-bottom: 10px;">Beliebte Mods</p>
                <p style="color: var(--text-secondary);">Gib einen Suchbegriff ein oder versuche es später erneut</p>
            </div>
        `;
    }
}

// Lädt die IDs der installierten Mods für das aktive Profil
async function loadInstalledModIds(force = false) {
    // NUR das aktuell ausgewählte Profil verwenden - kein Fallback!
    const profile = currentProfile;

    if (!profile) {
        debugLog('No profile selected - mods will not be marked as installed', 'info');
        installedModIds.clear();
        installedModIdsProfileId = null;
        return;
    }

    if (!force && installedModIdsProfileId === profile.id && installedModIds.size > 0) {
        return;
    }

    installedModIds.clear();

    try {
        const mods = await invoke('get_installed_mods', { profileId: profile.id });
        debugLog('Loading installed mod IDs from ' + mods.length + ' mods for profile: ' + profile.name, 'info');

        mods.forEach(mod => {
            // Die mod_id aus der Metadaten-Datei (Modrinth ID wie "AANobbMI")
            // Exakte Übereinstimmung – kein Fuzzy-Matching, um False Positives zu vermeiden
            if (mod.mod_id) {
                installedModIds.add(mod.mod_id.toLowerCase());
                debugLog('  Added mod_id: ' + mod.mod_id.toLowerCase(), 'info');
            }

            // Den vollständigen Slug/Namen als Fallback (z.B. "sodium" oder "distant-horizons")
            if (mod.name) {
                const cleanName = mod.name.toLowerCase().replace(/\s+/g, '-');
                installedModIds.add(cleanName);
            }
        });

        installedModIdsProfileId = profile.id;
        debugLog('Total installed mod IDs cached: ' + installedModIds.size + ' - ' + Array.from(installedModIds).join(', '), 'info');
    } catch (e) {
        debugLog('Could not load installed mods: ' + e, 'error');
    }
}

// Lädt die Namen/IDs der installierten Resource Packs für das aktive Profil
async function loadInstalledResourcePackNames(force = false) {
    const profile = currentProfile;
    if (!profile) {
        installedResourcePackNames.clear();
        installedResourcePackNamesProfileId = null;
        return;
    }

    if (!force && installedResourcePackNamesProfileId === profile.id && installedResourcePackNames.size > 0) {
        return;
    }

    installedResourcePackNames.clear();

    try {
        const packs = await invoke('get_installed_resourcepacks', { profileId: profile.id });
        packs.forEach(pack => addInstalledPackKeys(installedResourcePackNames, pack));
        installedResourcePackNamesProfileId = profile.id;
        debugLog('Loaded ' + installedResourcePackNames.size + ' installed resource pack keys', 'info');
    } catch (e) {
        debugLog('Could not load installed resource packs: ' + e, 'error');
    }
}

// Lädt die Namen/IDs der installierten Shader Packs für das aktive Profil
async function loadInstalledShaderPackNames(force = false) {
    const profile = currentProfile;
    if (!profile) {
        installedShaderPackNames.clear();
        installedShaderPackNamesProfileId = null;
        return;
    }

    if (!force && installedShaderPackNamesProfileId === profile.id && installedShaderPackNames.size > 0) {
        return;
    }

    installedShaderPackNames.clear();

    try {
        const packs = await invoke('get_installed_shaderpacks', { profileId: profile.id });
        packs.forEach(pack => addInstalledPackKeys(installedShaderPackNames, pack));
        installedShaderPackNamesProfileId = profile.id;
        debugLog('Loaded ' + installedShaderPackNames.size + ' installed shader pack keys', 'info');
    } catch (e) {
        debugLog('Could not load installed shader packs: ' + e, 'error');
    }
}

async function searchMods(query, page = 0) {
    if (serverBrowserContext) {
        return searchServerContent(query, page);
    }
    const modList = document.getElementById('mod-list');
    if (!modList) return;

    currentModSearchQuery = query || '';
    currentModPage = page;

    // Bei leerem Query beliebte Mods laden
    if (!query || query.length < 2) {
        loadPopularMods(page);
        return;
    }

    modList.innerHTML = '<div class="loading"><div class="spinner" style="margin: 20px auto;"></div><p>Suche...</p></div>';

    // Installierte Mods laden für Markierung
    await loadInstalledModIds();

    const cacheKey = buildBrowserCacheKey('mods', query, page);
    const cached = getCachedBrowserResults(cacheKey);
    if (cached) {
        renderMods(cached, page);
        return;
    }

    try {
        const mods = await invoke('search_mods', {
            query,
            gameVersion: selectedFilters.version || null,
            loader: selectedFilters.loader || null,
            categories: selectedFilters.categories.length > 0 ? selectedFilters.categories : null,
            sortBy: selectedFilters.sort || 'downloads',
            offset: page * getEffectiveLimit(),
            limit: getEffectiveLimit()
        });

        setCachedBrowserResults(cacheKey, mods);
        renderMods(mods, page);
    } catch (error) {
        debugLog('Search failed: ' + error, 'error');
        modList.innerHTML = '<div class="loading">Suche fehlgeschlagen: ' + error + '</div>';
    }
}

function renderMods(mods, page = 0) {
    const list = document.getElementById('mod-list');
    if (!list) return;

    // Speichere Scroll-Position
    const scrollTop = list.scrollTop;

    // Speichere Original-Länge für Pagination (ob es noch mehr Ergebnisse gibt)
    const originalCount = mods.length;

    // Wenn "Hide Installed" aktiv ist, installierte vorher herausfiltern
    if (selectedFilters.hideInstalled && serverBrowserContext) {
        mods = mods.filter(mod => !serverInstalledProjectIds.has(mod.id));
    } else if (selectedFilters.hideInstalled && currentProfile) {
        const beforeCount = mods.length;
        mods = mods.filter(mod => {
            const modSlug = normalizePackKey(mod.slug);
            const modName = normalizePackKey(mod.name);
            const modId = normalizePackKey(mod.id);
            const modFirstName = normalizePackKey(mod.name ? mod.name.split(' ')[0] : '');

            let isInst = false;
            if (currentContentType === 'mods') {
                // Nur exakte Treffer: Modrinth-Projekt-ID oder vollständiger Slug/Name
                // Kein modFirstName-Check – erzeugt False Positives bei Seite 2+
                isInst = installedModIds.has(modId) ||
                    installedModIds.has(modSlug) ||
                    installedModIds.has(modName);
            } else if (currentContentType === 'resourcepacks') {
                isInst = installedResourcePackNames.has(modId) ||
                    installedResourcePackNames.has(modSlug) ||
                    installedResourcePackNames.has(modName) ||
                    installedResourcePackNames.has(modFirstName);
            } else if (currentContentType === 'shaderpacks') {
                isInst = installedShaderPackNames.has(modId) ||
                    installedShaderPackNames.has(modSlug) ||
                    installedShaderPackNames.has(modName) ||
                    installedShaderPackNames.has(modFirstName);
            }
            return !isInst;
        });
        debugLog('Hide Installed: ' + beforeCount + ' -> ' + mods.length + ' (installedModIds: ' + installedModIds.size + ', profile: ' + (currentProfile ? currentProfile.name : 'none') + ')', 'info');
    }

    // Auf MODS_PER_PAGE begrenzen (wir haben evtl. mehr geladen wegen Overfetch)
    const hasMore = mods.length > MODS_PER_PAGE || originalCount >= getEffectiveLimit();
    mods = mods.slice(0, MODS_PER_PAGE);

    if (mods.length === 0 && page === 0) {
        list.innerHTML = '<div class="loading">Keine Inhalte gefunden</div>';
        return;
    }

    // Bestimme die richtige Install-Funktion basierend auf Content-Typ
    const getInstallFunction = () => {
        switch(currentContentType) {
            case 'resourcepacks': return 'installResourcePack';
            case 'shaderpacks': return 'installShaderPack';
            case 'modpacks': return 'installModpack'; // TODO: implementieren
            default: return 'installMod';
        }
    };

    const modsHTML = mods.map(mod => {
        // Prüfe ob bereits installiert ist - NUR wenn ein Profil ausgewählt ist
        let isInstalled = false;
        if (serverBrowserContext) {
            isInstalled = serverInstalledProjectIds.has(mod.id);
        } else if (currentProfile) {
            const modSlug = normalizePackKey(mod.slug);
            const modName = normalizePackKey(mod.name);
            const modId = normalizePackKey(mod.id);
            const modFirstName = normalizePackKey(mod.name ? mod.name.split(' ')[0] : '');

            if (currentContentType === 'mods') {
                // Nur exakte Treffer: Modrinth-Projekt-ID oder vollständiger Slug/Name
                // Kein modFirstName-Check – erzeugt False Positives bei Seite 2+
                isInstalled = installedModIds.has(modId) ||
                    installedModIds.has(modSlug) ||
                    installedModIds.has(modName);
            } else if (currentContentType === 'resourcepacks') {
                isInstalled = installedResourcePackNames.has(modId) ||
                    installedResourcePackNames.has(modSlug) ||
                    installedResourcePackNames.has(modName) ||
                    installedResourcePackNames.has(modFirstName);
            } else if (currentContentType === 'shaderpacks') {
                isInstalled = installedShaderPackNames.has(modId) ||
                    installedShaderPackNames.has(modSlug) ||
                    installedShaderPackNames.has(modName) ||
                    installedShaderPackNames.has(modFirstName);
            }
        }

        // Zeige Profil-Info im Button wenn kein Profil ausgewählt
        const installButtonText = (currentProfile || serverBrowserContext) ? 'Install' : 'Installieren...';
        const installFunc = serverBrowserContext ? 'installServerContentFromBrowser' : getInstallFunction();

        // Icon basierend auf Content-Typ
        const defaultIcon = serverBrowserContext?.projectType === 'plugin' ? '<i class="bi bi-plug"></i>' :
            currentContentType === 'resourcepacks' ? '<i class="bi bi-palette"></i>' :
            currentContentType === 'shaderpacks' ? '<i class="bi bi-stars"></i>' :
                currentContentType === 'modpacks' ? '<i class="bi bi-collection"></i>' : '<i class="bi bi-puzzle"></i>';
        const fallbackIconClass = currentContentType === 'resourcepacks' ? 'bi-palette' :
            currentContentType === 'shaderpacks' ? 'bi-stars' :
                currentContentType === 'modpacks' ? 'bi-collection' : 'bi-puzzle';

        // Erstelle Icon HTML mit Fallback - prüfe ob icon_url wirklich existiert und nicht leer ist
        const hasValidIcon = mod.icon_url && typeof mod.icon_url === 'string' && mod.icon_url.trim().length > 0;

        let iconHTML;
        if (hasValidIcon) {
            iconHTML = `<img src="${escapeAttr(mod.icon_url)}" alt="${escapeAttr(mod.name)}" 
                             style="width: 100%; height: 100%; object-fit: cover; border-radius: 8px;"
                             data-fallback-icon="${fallbackIconClass}"
                             data-fallback="icon-medium">`;
        } else {
            iconHTML = `<div style="font-size: 32px; display: flex; align-items: center; justify-content: center; width: 100%; height: 100%;">${defaultIcon}</div>`;
        }

        return `
            <div class="mod-card" data-mod-id="${mod.id}" data-mod-source="${mod.source || 'modrinth'}" 
                 style="${isInstalled ? 'opacity: 0.7; border-color: #555;' : ''} cursor: pointer;"
                 onclick="handleModCardClick(event, '${mod.id}', '${mod.source || 'modrinth'}')">
                <div class="mod-icon">
                    ${iconHTML}
                </div>
                <div class="mod-info">
                    <div style="display: flex; align-items: center; gap: 10px; margin-bottom: 5px;">
                        <span class="mod-name" style="font-size: 16px; font-weight: 600; color: var(--text-primary);">${mod.name}</span>
                        <span style="color: var(--text-secondary); font-size: 14px;">by</span>
                        <a href="https://modrinth.com/user/${mod.author}" 
                           target="_blank"
                           style="color: var(--text-secondary); font-size: 14px; text-decoration: underline; cursor: pointer; transition: color 0.2s;"
                           onmouseover="this.style.color='#999'"
                           onmouseout="this.style.color='var(--text-secondary)'">${mod.author}</a>
                        ${isInstalled ? '<span style="background: #4caf50; color: white; font-size: 10px; padding: 2px 5px; border-radius: 3px;"><i class="bi bi-check"></i> Installiert</span>' : ''}
                    </div>
                    <div class="mod-description" style="margin-bottom: 10px;">${mod.description}</div>
                    
                    <!-- Environment, Loader & Categories -->
                    <div style="display: flex; flex-wrap: wrap; gap: 6px; align-items: center;">
                        <!-- Environment -->
                        ${(() => {
            const clientSide = mod.client_side;
            const serverSide = mod.server_side;

            // Client & Server: beide sind "required"
            if (clientSide === 'required' && serverSide === 'required') {
                return `<span style="background: var(--bg-dark); color: var(--gold); font-size: 10px; padding: 3px 8px; border-radius: 4px; font-weight: 600; border: 1px solid var(--gold);">Client & Server</span>`;
            }
            // Client or Server: beide sind "optional" ODER einer required + einer optional
            else if ((clientSide === 'optional' && serverSide === 'optional') ||
                (clientSide === 'required' && serverSide === 'optional') ||
                (clientSide === 'optional' && serverSide === 'required')) {
                return `<span style="background: var(--bg-dark); color: var(--gold); font-size: 10px; padding: 3px 8px; border-radius: 4px; font-weight: 600; border: 1px solid var(--gold);">Client or Server</span>`;
            }
            // Nur Client: client=required/optional, server=unsupported/unknown
            else if ((clientSide === 'required' || clientSide === 'optional') &&
                (serverSide === 'unsupported' || serverSide === 'unknown' || !serverSide)) {
                return `<span style="background: var(--bg-dark); color: var(--gold); font-size: 10px; padding: 3px 8px; border-radius: 4px; font-weight: 600; border: 1px solid var(--gold);">Client</span>`;
            }
            // Nur Server: server=required/optional, client=unsupported/unknown
            else if ((serverSide === 'required' || serverSide === 'optional') &&
                (clientSide === 'unsupported' || clientSide === 'unknown' || !clientSide)) {
                return `<span style="background: var(--bg-dark); color: var(--gold); font-size: 10px; padding: 3px 8px; border-radius: 4px; font-weight: 600; border: 1px solid var(--gold);">Server</span>`;
            }
            return '';
        })()}
                        
                        <!-- Mod Loader -->
                        ${mod.loaders && mod.loaders.length > 0 ?
            mod.loaders.slice(0, 3).map(loader =>
                `<span style="background: var(--bg-light); color: var(--text-primary); font-size: 10px; padding: 3px 8px; border-radius: 4px; font-weight: 500;">${loader.charAt(0).toUpperCase() + loader.slice(1)}</span>`
            ).join('') : ''
        }
                        
                        <!-- Categories -->
                        ${mod.categories && mod.categories.length > 0 ?
            mod.categories.slice(0, 4).map(cat =>
                `<span style="background: var(--bg-light); color: var(--text-secondary); font-size: 10px; padding: 3px 8px; border-radius: 4px;">${cat}</span>`
            ).join('') : ''
        }
                    </div>
                </div>
                
                <!-- Button und Downloads rechts (vertikal) -->
                <div style="display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 8px;">
                    ${isInstalled
            ? `<button class="btn btn-secondary" disabled style="opacity: 0.5; cursor: not-allowed;">Installiert</button>`
            : `<button class="btn install-btn" data-mod-id="${mod.id}" onclick="${installFunc}('${mod.id}', '${mod.source}')">${installButtonText}</button>`
        }
                    <div style="color: var(--text-secondary); font-size: 14px; white-space: nowrap; text-align: center;">
                        <span style="font-weight: bold; color: var(--text-primary);">${formatNumber(mod.downloads)}</span>
                        <span style="font-weight: 300;"> downloads</span>
                    </div>
                </div>
            </div>
        `;
    }).join('');

    // Pagination Buttons
    const paginationHTML = `
        <div style="display: flex; justify-content: center; align-items: center; gap: 15px; padding: 20px; background: var(--bg-medium); border-radius: 8px; margin-top: 10px;">
            <button class="btn btn-secondary" onclick="previousModPage()" 
                    ${page === 0 ? 'disabled style="opacity: 0.5; cursor: not-allowed;"' : ''}>
                ← Vorherige
            </button>
            <span style="color: var(--text-secondary); font-size: 14px;">
                Seite ${page + 1}
            </span>
            <button class="btn btn-secondary" onclick="nextModPage()" 
                    ${!hasMore ? 'disabled style="opacity: 0.5; cursor: not-allowed;"' : ''}>
                Nächste →
            </button>
        </div>
    `;

    list.innerHTML = modsHTML + paginationHTML;

    // Stelle Scroll-Position wieder her
    list.scrollTop = scrollTop;
}

function previousModPage() {
    if (currentModPage > 0) {
        const query = currentModSearchQuery;
        const prevPage = currentModPage - 1;

        if (query) {
            if (currentContentType === 'mods') searchMods(query, prevPage);
            else if (currentContentType === 'resourcepacks') searchResourcePacks(query, prevPage);
            else if (currentContentType === 'shaderpacks') searchShaderPacks(query, prevPage);
            else if (currentContentType === 'modpacks') searchModpacks(query, prevPage);
        } else {
            if (currentContentType === 'mods') loadPopularMods(prevPage);
            else if (currentContentType === 'resourcepacks') loadPopularResourcePacks(prevPage);
            else if (currentContentType === 'shaderpacks') loadPopularShaderPacks(prevPage);
            else if (currentContentType === 'modpacks') loadPopularModpacks(prevPage);
        }
    }
}

function nextModPage() {
    const query = currentModSearchQuery;
    const nextPage = currentModPage + 1;

    if (query) {
        if (currentContentType === 'mods') searchMods(query, nextPage);
        else if (currentContentType === 'resourcepacks') searchResourcePacks(query, nextPage);
        else if (currentContentType === 'shaderpacks') searchShaderPacks(query, nextPage);
        else if (currentContentType === 'modpacks') searchModpacks(query, nextPage);
    } else {
        if (currentContentType === 'mods') loadPopularMods(nextPage);
        else if (currentContentType === 'resourcepacks') loadPopularResourcePacks(nextPage);
        else if (currentContentType === 'shaderpacks') loadPopularShaderPacks(nextPage);
        else if (currentContentType === 'modpacks') loadPopularModpacks(nextPage);
    }
}

async function installMod(modId, source) {
    // Verwende das aktuell ausgewählte Profil oder zeige Auswahl
    let profile = currentProfile;

    if (!profile) {
        // Zeige Profil-Auswahl Dialog
        if (profiles.length === 0) {
            showToast('Bitte erstelle zuerst ein Profil!', 'warning', 4000);
            switchPage('profiles');
            return;
        }

        profile = await showProfileSelectDialog();
        if (!profile) {
            return; // Abgebrochen
        }
    }

    debugLog('Installing mod ' + modId + ' to profile ' + profile.name + ' (' + profile.minecraft_version + ' ' + profile.loader.loader + ')', 'info');

    // Markiere Button als "installierend"
    const btn = event.target;
    const originalText = btn.textContent;
    btn.textContent = '...';
    btn.disabled = true;

    try {
        // Installiere Mod - Backend findet automatisch die passende Version für das Profil
        await invoke('install_mod', {
            profileId: profile.id,
            modId: modId,
            versionId: null,  // Backend findet passende Version für MC-Version + Loader
            source: source
        });

        debugLog('Mod installed successfully!', 'success');

        // Button als installiert markieren mit korrektem Styling
        btn.innerHTML = '<i class="bi bi-check"></i> Installiert';
        btn.disabled = true;
        btn.style.background = 'var(--bg-light)';
        btn.style.color = 'var(--text-secondary)';
        btn.style.opacity = '0.7';
        btn.style.cursor = 'not-allowed';

        // Aktualisiere Cache für installierte Mods (im Hintergrund)
        invalidateProfileContentCaches(profile.id);
        loadInstalledModIds(true);

        // Toast-Benachrichtigung
        showToast(`Mod erfolgreich zu "${profile.name}" hinzugefügt!`, 'success', 3000);

    } catch (error) {
        debugLog('Install failed: ' + error, 'error');

        // Button zurücksetzen bei Fehler
        btn.textContent = originalText;
        btn.disabled = false;

        // Toast-Benachrichtigung für Fehler
        showToast('Mod-Installation fehlgeschlagen: ' + error, 'error', 3000);
    }
}

function showModInstallError(title, htmlContent) {
    const modalHTML = `
        <div style="position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.8); display: flex; align-items: center; justify-content: center; z-index: 10000;" onclick="this.remove()">
            <div style="background: var(--bg-dark); border: 2px solid #f44336; border-radius: 10px; padding: 30px; max-width: 500px;" onclick="event.stopPropagation()">
                <h2 style="color: #f44336; margin: 0 0 20px 0;"><i class="bi bi-x-circle-fill"></i> ${title}</h2>
                <div style="color: var(--text-secondary); line-height: 1.6;">
                    ${htmlContent}
                </div>
                <button class="btn btn-secondary" onclick="this.closest('div[style*=\\'position: fixed\\']').remove()" style="width: 100%; margin-top: 20px; padding: 12px;">
                    Schließen
                </button>
            </div>
        </div>
    `;

    const modalDiv = document.createElement('div');
    modalDiv.innerHTML = modalHTML;
    document.body.appendChild(modalDiv.firstElementChild);
}

// Profil-Auswahl Dialog für Mod-Installation
function showProfileSelectDialog() {
    return new Promise((resolve) => {
        const modalHTML = `
            <div id="profile-select-modal" style="position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.85); display: flex; align-items: center; justify-content: center; z-index: 10000;">
                <div style="background: var(--bg-dark); border: 2px solid var(--gold); border-radius: 12px; padding: 25px; max-width: 450px; width: 90%;">
                    <h2 style="color: var(--gold); margin: 0 0 20px 0; text-align: center;"><i class="bi bi-person-circle"></i> Profil auswählen</h2>
                    <p style="color: var(--text-secondary); text-align: center; margin-bottom: 20px;">
                        Wähle ein Profil für die Mod-Installation:
                    </p>
                    <div style="max-height: 300px; overflow-y: auto; display: flex; flex-direction: column; gap: 10px;">
                        ${profiles.map(p => `
                            <div class="profile-select-option" 
                                 onclick="selectProfileForInstall('${p.id}')"
                                 style="background: var(--bg-light); padding: 15px; border-radius: 8px; cursor: pointer; 
                                        display: flex; align-items: center; gap: 15px; transition: all 0.2s;
                                        border: 2px solid transparent;"
                                 onmouseover="this.style.borderColor='var(--gold)'"
                                 onmouseout="this.style.borderColor='transparent'">
                                <div style="font-size: 24px;"><i class="bi bi-box"></i></div>
                                <div style="flex: 1;">
                                    <div style="color: var(--text-primary); font-weight: bold;">${p.name}</div>
                                    <div style="color: var(--text-secondary); font-size: 12px;">
                                        ${p.loader.loader} ${p.minecraft_version}
                                    </div>
                                </div>
                            </div>
                        `).join('')}
                    </div>
                    <button class="btn btn-secondary" onclick="cancelProfileSelect()" 
                            style="width: 100%; margin-top: 20px; padding: 12px;">
                        Cancel
                    </button>
                </div>
            </div>
        `;

        const modalDiv = document.createElement('div');
        modalDiv.innerHTML = modalHTML;
        document.body.appendChild(modalDiv.firstElementChild);

        // Speichere resolve-Funktion global für die Callbacks
        window._profileSelectResolve = resolve;
    });
}

function selectProfileForInstall(profileId) {
    const profile = profiles.find(p => p.id === profileId);
    const modal = document.getElementById('profile-select-modal');
    if (modal) modal.remove();

    if (window._profileSelectResolve) {
        window._profileSelectResolve(profile);
        window._profileSelectResolve = null;
    }
}

function cancelProfileSelect() {
    const modal = document.getElementById('profile-select-modal');
    if (modal) modal.remove();

    if (window._profileSelectResolve) {
        window._profileSelectResolve(null);
        window._profileSelectResolve = null;
    }
}

// Settings
function normalizeFsPath(value) {
    return String(value || '')
        .trim()
        .replace(/\//g, '\\')
        .toLowerCase();
}

function updateJavaRuntimeHint() {
    const select = document.getElementById('settings-java-runtime');
    const hint = document.getElementById('settings-java-runtime-hint');
    if (!select || !hint) return;

    const selected = javaRuntimeOptionsCache.find(o => String(o.value) === String(select.value));
    if (!selected) {
        hint.textContent = 'Lion Launcher downloads the selected Java version automatically if needed.';
        return;
    }

    if (selected.value === 'auto') {
        hint.textContent = `Automatic mode uses the latest stable JDK (currently ${selected.major}).`;
        return;
    }

    hint.textContent = selected.installed
        ? `Java ${selected.major} is already available on this machine.`
        : `Java ${selected.major} will be downloaded when you save settings.`;
}

async function populateJavaRuntimeOptions(currentJavaPath = null) {
    const select = document.getElementById('settings-java-runtime');
    const customInput = document.getElementById('settings-java-custom-path');
    if (!select) return;

    try {
        const [latestMajor, options] = await Promise.all([
            invoke('get_latest_stable_jdk_major').catch(() => 21),
            invoke('get_java_runtime_options').catch(() => []),
        ]);

        latestStableJavaMajor = Number(latestMajor) || 21;
        javaRuntimeOptionsCache = Array.isArray(options) ? options : [];

        if (!javaRuntimeOptionsCache.length) {
            javaRuntimeOptionsCache = [
                {
                    value: 'auto',
                    label: `Latest stable JDK (${latestStableJavaMajor}) - automatic`,
                    major: latestStableJavaMajor,
                    installed: false,
                    path: null,
                },
            ];
        }

        select.innerHTML = javaRuntimeOptionsCache.map(opt => {
            const badge = opt.value === 'auto' ? 'recommended' : (opt.installed ? 'installed' : 'download');
            return `<option value="${escapeAttr(String(opt.value))}">Java ${escapeHtml(String(opt.major))} - ${escapeHtml(String(opt.label))} (${badge})</option>`;
        }).join('');

        const preferred = localStorage.getItem('preferredJavaRuntime') || 'auto';
        const hasPreferred = javaRuntimeOptionsCache.some(opt => String(opt.value) === preferred);
        select.value = hasPreferred ? preferred : 'auto';

        if (currentJavaPath && customInput) {
            const normalizedCurrent = normalizeFsPath(currentJavaPath);
            const matched = javaRuntimeOptionsCache.some(opt => {
                if (!opt.path) return false;
                return normalizeFsPath(opt.path) === normalizedCurrent;
            });
            if (!matched) {
                customInput.value = currentJavaPath;
            }
        }

        updateJavaRuntimeHint();
    } catch (error) {
        debugLog('Failed to populate Java runtime options: ' + error, 'warning');
    }
}

async function loadSettings() {
    const username = localStorage.getItem('username') || 'Guest';
    currentUsername = username;

    const usernameDisplay = document.getElementById('username-display');
    if (usernameDisplay) {
        usernameDisplay.textContent = t('player_label') + username;
    }

    let config = null;
    try {
        config = await invoke('get_config');
    } catch (error) {
        debugLog('Failed to load backend config: ' + error, 'warning');
    }

    const memory = String(config?.game_settings?.memory_mb || localStorage.getItem('memory') || '4096');
    const memoryInput = document.getElementById('settings-memory');
    if (memoryInput) {
        memoryInput.value = memory;
    }

    const customJavaInput = document.getElementById('settings-java-custom-path');
    if (customJavaInput) {
        customJavaInput.value = config?.game_settings?.java_path || '';
    }

    await populateJavaRuntimeOptions(config?.game_settings?.java_path || null);

    const javaRuntimeSelect = document.getElementById('settings-java-runtime');
    if (javaRuntimeSelect && !javaRuntimeSelect.dataset.bound) {
        javaRuntimeSelect.addEventListener('change', () => {
            localStorage.setItem('preferredJavaRuntime', javaRuntimeSelect.value || 'auto');
            updateJavaRuntimeHint();
        });
        javaRuntimeSelect.dataset.bound = '1';
    }

    // Theme laden
    const savedTheme = localStorage.getItem('theme') || 'dark';
    const savedAccent = localStorage.getItem('accentColor') || 'gold';
    setTheme(savedTheme, false);
    setAccentColor(savedAccent, false);
}

// Theme Functions
function setTheme(theme, save = true) {
    currentTheme = theme;
    document.documentElement.setAttribute('data-theme', theme);

    // Button-Styles aktualisieren
    const darkBtn = document.getElementById('theme-dark-btn');
    const lightBtn = document.getElementById('theme-light-btn');

    if (darkBtn && lightBtn) {
        if (theme === 'dark') {
            darkBtn.style.borderColor = 'var(--gold)';
            lightBtn.style.borderColor = 'transparent';
        } else {
            darkBtn.style.borderColor = 'transparent';
            lightBtn.style.borderColor = 'var(--gold)';
        }
    }

    if (save) {
        localStorage.setItem('theme', theme);
        showToast(`${t('theme_changed')} ${theme === 'dark' ? t('toast_theme_dark') : t('toast_theme_light')}`, 'success', 2000);
    }
}

function setAccentColor(color, save = true) {
    currentAccentColor = color;
    document.documentElement.setAttribute('data-accent', color);

    // Farbauswahl-Styles aktualisieren
    document.querySelectorAll('.color-option').forEach(opt => {
        if (opt.dataset.color === color) {
            opt.style.borderColor = 'var(--gold)';
            opt.style.transform = 'scale(1.1)';
        } else {
            opt.style.borderColor = 'transparent';
            opt.style.transform = 'scale(1)';
        }
    });

    if (save) {
        localStorage.setItem('accentColor', color);
        showToast(t('toast_accent_changed'), 'success', 2000);
    }
}

/** Lädt die Launcher-Konfiguration; wirft, wenn sie unvollständig ist. */
async function loadGameConfig() {
    const config = await invoke('get_config');
    if (!config || !config.game_settings) {
        throw new Error('Config konnte nicht geladen werden');
    }
    return config;
}

/**
 * Ermittelt den zu speichernden Java-Pfad:
 * eigener Pfad > Pfad der gewählten Runtime > automatisch bereitgestellte Runtime.
 */
async function resolveJavaPathForSettings(customJavaPath, selectedRuntimeValue) {
    if (customJavaPath) return customJavaPath;

    const selectedOption = javaRuntimeOptionsCache.find(o => String(o.value) === selectedRuntimeValue);
    if (selectedOption?.path) return selectedOption.path;

    let targetMajor = latestStableJavaMajor;
    if (selectedOption) {
        targetMajor = Number(selectedOption.major) || latestStableJavaMajor;
    } else if (selectedRuntimeValue !== 'auto') {
        const parsed = parseInt(selectedRuntimeValue, 10);
        if (Number.isFinite(parsed) && parsed > 0) targetMajor = parsed;
    }

    showToast(`Preparing Java ${targetMajor}...`, 'info', 2500);
    return invoke('ensure_java_runtime_for_settings', { major: targetMajor });
}

const saveSettingsBtn = document.getElementById('save-settings-btn');
if (saveSettingsBtn) {
    saveSettingsBtn.addEventListener('click', async () => {
        const memoryInput = document.getElementById('settings-memory');
        const languageSelect = document.getElementById('settings-language');
        const javaRuntimeSelect = document.getElementById('settings-java-runtime');
        const customJavaInput = document.getElementById('settings-java-custom-path');

        if (!memoryInput) return;

        try {
            const memory = Math.max(512, parseInt(memoryInput.value || '4096', 10) || 4096);
            localStorage.setItem('memory', String(memory));

            const config = await loadGameConfig();

            const selectedRuntimeValue = javaRuntimeSelect?.value || 'auto';
            localStorage.setItem('preferredJavaRuntime', selectedRuntimeValue);
            const javaPathToSave = await resolveJavaPathForSettings(
                customJavaInput?.value.trim() || '',
                selectedRuntimeValue
            );

            config.game_settings.memory_mb = memory;
            config.game_settings.java_path = javaPathToSave || null;

            await invoke('save_config', { config });

            await populateJavaRuntimeOptions(javaPathToSave || null);

            // Language change: save and reload
            if (languageSelect) {
                const newLang = languageSelect.value;
                const oldLang = localStorage.getItem('language') || 'en';
                localStorage.setItem('language', newLang);
                if (newLang !== oldLang) {
                    showToast(t('toast_settings_saved'), 'success', 1500);
                    setTimeout(() => window.location.reload(), 1200);
                    return;
                }
            }

            showToast(t('toast_settings_saved'), 'success', 3000);
        } catch (error) {
            showToast('Settings konnten nicht gespeichert werden: ' + error, 'error', 4200);
        }
    });
}

// ==================== ACCOUNT MANAGEMENT ====================

let activeAccount = null;
let recentSkins = JSON.parse(localStorage.getItem('recentSkins') || '[]');
let savedSkins = JSON.parse(localStorage.getItem('savedSkins') || '[]');

async function loadAccounts() {
    try {
        // Aktiven Account laden
        const active = await invoke('get_active_account');
        activeAccount = active;

        updateAccountDisplay();
        updateAccountsList();
        updateSkinPage();

        if (active) {
            currentUsername = active.username;
            debugLog('Loaded active account: ' + active.username, 'success');
        }
    } catch (error) {
        debugLog('Failed to load accounts: ' + error, 'error');
    }
}

function updateAccountDisplay() {
    const headImg = document.getElementById('account-head');
    const nameEl = document.getElementById('account-name');
    const typeEl = document.getElementById('account-type');

    if (activeAccount) {
        if (headImg) headImg.src = activeAccount.head_url;
        if (nameEl) nameEl.textContent = activeAccount.username;
        if (typeEl) typeEl.innerHTML = activeAccount.is_microsoft ? '<i class="bi bi-shield-lock"></i> Microsoft' : '<i class="bi bi-person"></i> Offline';
    } else {
        if (headImg) headImg.src = 'https://mc-heads.net/avatar/MHF_Steve/40';
        if (nameEl) nameEl.textContent = 'Nicht angemeldet';
        if (typeEl) typeEl.textContent = 'Klicke zum Anmelden';
    }
}

async function updateAccountsList() {
    const list = document.getElementById('accounts-list');
    const activeDisplay = document.getElementById('active-account-display');
    if (!list) return;

    try {
        const accounts = await invoke('get_accounts');

        // Aktiver Account anzeigen
        if (activeAccount && activeDisplay) {
            activeDisplay.innerHTML = `
                <div style="display: flex; align-items: center; gap: 15px;">
                    <img alt="" src="${activeAccount.head_url}" style="width: 64px; height: 64px; border-radius: 8px; image-rendering: pixelated;">
                    <div style="flex: 1;">
                        <p style="margin: 0; color: var(--text-primary); font-weight: bold; font-size: 18px;">${activeAccount.username}</p>
                        <p style="margin: 5px 0 0 0; color: var(--text-secondary); font-size: 12px;">
                            ${activeAccount.is_microsoft ? '<i class="bi bi-shield-lock"></i> Microsoft Account' : '<i class="bi bi-person"></i> Offline Account'}
                        </p>
                    </div>
                    <button class="btn btn-secondary" onclick="logoutAccount('${activeAccount.uuid}')" style="padding: 8px 15px; font-size: 12px;">
                        Abmelden
                    </button>
                </div>
            `;
        } else if (activeDisplay) {
            activeDisplay.innerHTML = `
                <div style="text-align: center; color: var(--text-secondary); padding: 20px;">
                    <div style="font-size: 32px; margin-bottom: 10px;"><i class="bi bi-person"></i></div>
                    <p>Kein Account angemeldet</p>
                    <p style="font-size: 12px;">Melde dich mit Microsoft an oder erstelle einen Offline-Account</p>
                </div>
            `;
        }

        // Liste aller Accounts
        if (accounts.length > 1) {
            const otherAccounts = accounts.filter(a => !a.is_active);
            list.innerHTML = `
                <h4 style="color: var(--text-secondary); margin: 0 0 10px 0; font-size: 14px;">Weitere Accounts</h4>
                ${otherAccounts.map(acc => `
                    <div style="display: flex; align-items: center; gap: 10px; padding: 10px; background: var(--bg-dark); border-radius: 8px; margin-bottom: 8px;">
                        <img alt="" src="${acc.head_url}" style="width: 32px; height: 32px; border-radius: 4px; image-rendering: pixelated;">
                        <span style="flex: 1; color: var(--text-primary);">${acc.username}</span>
                        <span style="color: var(--text-secondary); font-size: 11px;">${acc.is_microsoft ? '<i class="bi bi-shield-lock"></i>' : '<i class="bi bi-person"></i>'}</span>
                        <button class="btn btn-secondary" onclick="switchAccount('${acc.uuid}')" style="padding: 5px 10px; font-size: 11px;">
                            Wechseln
                        </button>
                        <button class="btn btn-secondary" onclick="logoutAccount('${acc.uuid}')" style="padding: 5px 10px; font-size: 11px; color: #f44336;">
                            <i class="bi bi-x"></i>
                        </button>
                    </div>
                `).join('')}
            `;
        } else {
            list.innerHTML = '';
        }
    } catch (error) {
        debugLog('Failed to update accounts list: ' + error, 'error');
    }
}

async function startMicrosoftLogin() {
    debugLog('Starting Microsoft login with Device Code Flow...', 'info');

    try {
        const flow = await invoke('begin_microsoft_login');
        debugLog('Got device code: ' + flow.user_code, 'info');

        // Öffne Browser mit Verification URL
        await invoke('open_auth_url', { url: flow.verification_uri });

        // Zeige Modal mit Code
        const modalHTML = `
            <div id="microsoft-login-modal" style="position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.9); display: flex; align-items: center; justify-content: center; z-index: 10000;">
                <div style="background: var(--bg-dark); border: 2px solid var(--gold); border-radius: 10px; padding: 30px; min-width: 450px; max-width: 500px; text-align: center;">
                    <h3 style="color: var(--gold); margin: 0 0 25px 0;"><i class="bi bi-shield-lock"></i> Microsoft Login</h3>
                    
                    <div style="background: var(--bg-light); border-radius: 8px; padding: 20px; margin-bottom: 25px;">
                        <p style="color: var(--text-secondary); margin: 0 0 15px 0; font-size: 14px;">
                            Ein Browser-Fenster wurde geöffnet.<br>
                            Gib dort diesen Code ein:
                        </p>
                        <div style="background: var(--bg-dark); border: 3px solid var(--gold); border-radius: 8px; padding: 20px; margin: 10px 0;">
                            <span id="device-code-display" style="font-size: 32px; font-weight: bold; color: var(--gold); letter-spacing: 5px; font-family: monospace;">
                                ${flow.user_code}
                            </span>
                        </div>
                        <button onclick="navigator.clipboard.writeText('${flow.user_code}'); showToast('Code kopiert!', 'success', 2000);" 
                                class="btn btn-secondary" style="margin-top: 10px; padding: 8px 20px;">
                            <i class="bi bi-clipboard"></i> Code kopieren
                        </button>
                    </div>
                    
                    <div id="login-status" style="margin-bottom: 20px;">
                        <div class="spinner" style="margin: 0 auto 10px;"></div>
                        <p style="color: var(--text-secondary); margin: 0;">Warte auf Anmeldung...</p>
                    </div>
                    
                    <button class="btn btn-secondary" onclick="cancelMicrosoftLogin()" style="padding: 10px 30px;">
                        Cancel
                    </button>
                </div>
            </div>
        `;

        document.body.insertAdjacentHTML('beforeend', modalHTML);

        showToast('Gib den Code im Browser ein: ' + flow.user_code, 'info', 10000);

        // Polling für Token
        currentDeviceCode = flow.device_code;
        pollForMicrosoftToken(flow.device_code, flow.interval || 5);

    } catch (error) {
        debugLog('Microsoft login failed: ' + error, 'error');
        showToast('Login fehlgeschlagen: ' + error, 'error', 5000);
    }
}

let currentDeviceCode = null;
let pollingInterval = null;

async function pollForMicrosoftToken(deviceCode, interval) {
    pollingInterval = setInterval(async () => {
        try {
            const result = await invoke('poll_microsoft_login', { deviceCode });

            if (result) {
                // Login erfolgreich!
                clearInterval(pollingInterval);
                pollingInterval = null;
                currentDeviceCode = null;

                activeAccount = result;
                currentUsername = result.username;

                document.getElementById('microsoft-login-modal')?.remove();

                updateAccountDisplay();
                updateAccountsList();
                updateSkinPage();

                showToast(`Willkommen, ${result.username}!`, 'success', 3000);
                debugLog('Microsoft login successful: ' + result.username, 'success');
            }
        } catch (error) {
            clearInterval(pollingInterval);
            pollingInterval = null;
            currentDeviceCode = null;

            document.getElementById('microsoft-login-modal')?.remove();

            debugLog('Microsoft login error: ' + error, 'error');
            showToast('Login fehlgeschlagen: ' + error, 'error', 5000);
        }
    }, interval * 1000);
}

function cancelMicrosoftLogin() {
    if (pollingInterval) {
        clearInterval(pollingInterval);
        pollingInterval = null;
    }
    currentDeviceCode = null;
    document.getElementById('microsoft-login-modal')?.remove();
    showToast('Login abgebrochen', 'warning', 3000);
}

function showOfflineAccountModal() {
    const modalHTML = `
        <div id="offline-account-modal" style="position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.8); display: flex; align-items: center; justify-content: center; z-index: 10000;" onclick="if(event.target === this) this.remove()">
            <div style="background: var(--bg-dark); border: 2px solid var(--gold); border-radius: 10px; padding: 30px; min-width: 400px;" onclick="event.stopPropagation()">
                <h3 style="color: var(--gold); margin: 0 0 20px 0;"><i class="bi bi-person"></i> Offline Account erstellen</h3>
                <div style="margin-bottom: 20px;">
                    <label style="display: block; margin-bottom: 8px; color: var(--text-secondary);">Spielername</label>
                    <input type="text" id="offline-username" placeholder="Dein Minecraft-Name" 
                           style="width: 100%; padding: 12px; background: var(--bg-light); border: 2px solid var(--bg-light); border-radius: 8px; color: var(--text-primary);"
                           maxlength="16">
                    <p style="color: var(--text-secondary); font-size: 11px; margin-top: 5px;">
                        <i class="bi bi-exclamation-triangle-fill" style="color:#ff9800;"></i> Offline-Accounts können nur auf Servern mit deaktivierter Online-Authentifizierung spielen
                    </p>
                </div>
                <div style="display: flex; gap: 10px;">
                    <button class="btn btn-secondary" onclick="document.getElementById('offline-account-modal').remove()" style="flex: 1;">
                        Cancel
                    </button>
                    <button class="btn" onclick="createOfflineAccount()" style="flex: 1;">
                        Erstellen
                    </button>
                </div>
            </div>
        </div>
    `;
    document.body.insertAdjacentHTML('beforeend', modalHTML);
    document.getElementById('offline-username').focus();
}

async function createOfflineAccount() {
    const username = document.getElementById('offline-username').value.trim();

    if (!username) {
        showToast('Bitte gib einen Spielernamen ein', 'warning', 3000);
        return;
    }

    try {
        const account = await invoke('add_offline_account', { username });
        activeAccount = account;
        currentUsername = account.username;

        document.getElementById('offline-account-modal')?.remove();

        updateAccountDisplay();
        updateAccountsList();
        updateSkinPage();

        showToast(`Offline-Account "${username}" erstellt!`, 'success', 3000);
    } catch (error) {
        showToast('Fehler: ' + error, 'error', 3000);
    }
}

async function switchAccount(uuid) {
    try {
        await invoke('set_active_account', { uuid });
        await loadAccounts();
        showToast('Account gewechselt!', 'success', 3000);
    } catch (error) {
        showToast('Fehler beim Wechseln: ' + error, 'error', 3000);
    }
}

async function logoutAccount(uuid) {
    const confirmed = await showConfirmModal({
        title: 'Abmelden',
        message: 'Möchtest du diesen Account wirklich abmelden',
        confirmText: 'Abmelden',
        icon: 'bi-box-arrow-right'
    });
    if (!confirmed) return;

    try {
        await invoke('remove_account', { uuid });

        if (activeAccount && activeAccount.uuid === uuid) {
            activeAccount = null;
            currentUsername = 'Guest';
        }

        await loadAccounts();
        showToast('Account abgemeldet', 'success', 3000);
    } catch (error) {
        showToast('Fehler: ' + error, 'error', 3000);
    }
}

// ==================== SKIN VIEWER (3D) ====================

let skinViewer = null;
let currentViewingUuid = null;  // UUID des aktuell angezeigten Skins (null = eigener)
let currentViewingSkinUrl = null; // Skin-URL für "Übernehmen"
let skinViewerInitialized = false;
let skinDefaultRotationY = 0;
let skinDefaultRotationX = 0;

function initSkinViewer() {
    if (skinViewerInitialized && skinViewer) return;
    
    const container = document.getElementById('skin-viewer');
    const canvas = document.getElementById('skin-canvas');
    if (!container || !canvas || typeof skinview3d === 'undefined') {
        debugLog('skinview3d not available, falling back to static image', 'warning');
        return;
    }

    // Nicht initialisieren wenn die Seite versteckt ist (0-Größe)
    const rect = container.getBoundingClientRect();
    if (rect.width < 10 || rect.height < 10) {
        debugLog('Skin viewer container not visible yet, deferring init', 'info');
        return;
    }

    try {
        // Zerstöre alten Viewer wenn vorhanden
        if (skinViewer) {
            skinViewer.dispose();
            skinViewer = null;
        }

        const width = Math.max(Math.min(Math.floor(rect.width) - 20, 280), 100);
        const height = Math.max(Math.min(Math.floor(rect.height) - 10, 400), 200);

        skinViewer = new skinview3d.SkinViewer({
            canvas: canvas,
            width: width,
            height: height,
        });

        // Idle-Animation
        skinViewer.animation = new skinview3d.IdleAnimation();
        skinViewer.animation.speed = 0.5;

        // Zoom deaktivieren, Rotation erlauben (eingebaute Controls in v3.4.x)
        skinViewer.controls.enableZoom = false;
        skinViewer.controls.enablePan = false;
        skinViewer.controls.enableRotate = true;
        skinViewer.controls.enableDamping = true;
        skinViewer.controls.dampingFactor = 0.1;
        
        // Speichere Default-Kamera-Position für Reset
        const defaultCamPos = {
            x: skinViewer.camera.position.x,
            y: skinViewer.camera.position.y,
            z: skinViewer.camera.position.z
        };
        
        // OrbitControls "saveState" für Reset
        if (skinViewer.controls.saveState) {
            skinViewer.controls.saveState();
        }

        // Rotation zurücksetzen wenn Maus losgelassen wird
        let resetTimeout = null;
        let isDragging = false;
        
        function scheduleReset(delay) {
            isDragging = false;
            if (resetTimeout) clearTimeout(resetTimeout);
            resetTimeout = setTimeout(() => {
                animateResetRotation(defaultCamPos);
            }, delay);
        }
        
        canvas.addEventListener('mousedown', () => { isDragging = true; if (resetTimeout) clearTimeout(resetTimeout); });
        canvas.addEventListener('mouseup', () => scheduleReset(200));
        canvas.addEventListener('mouseleave', () => { if (isDragging) scheduleReset(300); });
        canvas.addEventListener('touchstart', () => { isDragging = true; if (resetTimeout) clearTimeout(resetTimeout); });
        canvas.addEventListener('touchend', () => scheduleReset(200));

        skinViewerInitialized = true;
        debugLog('skinview3d 3D viewer initialized (' + width + 'x' + height + ')', 'success');
    } catch (e) {
        debugLog('Failed to init skinview3d: ' + e, 'error');
        skinViewerInitialized = false;
    }
}

function animateResetRotation(defaultCamPos) {
    if (!skinViewer) return;
    
    const cam = skinViewer.camera;
    const startX = cam.position.x;
    const startY = cam.position.y;
    const startZ = cam.position.z;
    const targetX = defaultCamPos ? defaultCamPos.x : 0;
    const targetY = defaultCamPos ? defaultCamPos.y : 0;
    const targetZ = defaultCamPos ? defaultCamPos.z : 60;
    
    // Wenn schon nahe genug, direkt reset
    const dist = Math.sqrt(
        Math.pow(startX - targetX, 2) + 
        Math.pow(startY - targetY, 2) + 
        Math.pow(startZ - targetZ, 2)
    );
    if (dist < 0.1) return;
    
    const duration = 500; // ms
    const startTime = performance.now();
    
    // Disable controls während der Animation
    if (skinViewer.controls) {
        skinViewer.controls.enableRotate = false;
    }
    
    function animStep(now) {
        if (!skinViewer) return;
        
        const elapsed = now - startTime;
        const progress = Math.min(elapsed / duration, 1);
        // Ease out cubic
        const ease = 1 - Math.pow(1 - progress, 3);
        
        cam.position.x = startX + (targetX - startX) * ease;
        cam.position.y = startY + (targetY - startY) * ease;
        cam.position.z = startZ + (targetZ - startZ) * ease;
        cam.lookAt(0, 0, 0);
        
        if (progress < 1) {
            requestAnimationFrame(animStep);
        } else {
            cam.position.set(targetX, targetY, targetZ);
            cam.lookAt(0, 0, 0);
            // Controls wieder aktivieren und zurücksetzen
            if (skinViewer.controls) {
                if (skinViewer.controls.reset) {
                    skinViewer.controls.reset();
                }
                skinViewer.controls.enableRotate = true;
            }
        }
    }
    
    requestAnimationFrame(animStep);
}

async function loadSkinToViewer(uuid) {
    if (!skinViewer) {
        initSkinViewer();
    }
    
    if (skinViewer) {
        try {
            // Skin-Textur über Rust proxen (CORS-sicher)
            const skinDataUrl = await invoke('get_skin_texture', { uuid: uuid });
            skinViewer.loadSkin(skinDataUrl);
        } catch (e) {
            debugLog('Skin proxy failed, trying direct URL: ' + e, 'warning');
            try {
                skinViewer.loadSkin(`https://mc-heads.net/skin/${uuid}`);
            } catch (e2) {
                debugLog('Direct skin load also failed: ' + e2, 'error');
            }
        }
    }
}

/**
 * Rendert den Kopf-Avatar direkt aus einer Skin-Textur (Data-URL oder Image-URL).
 * Extrahiert das 8x8 Gesicht (Position 8,8) + Overlay (40,8) und skaliert es hoch.
 * Aktualisiert sofort alle Avatar-Elemente im UI – kein mc-heads.net Cache nötig!
 */
function updateAvatarsFromSkinData(skinDataUrl) {
    const img = new Image();
    img.crossOrigin = 'anonymous';
    img.onload = () => {
        try {
            // Canvas für Kopf-Extraktion
            const canvas = document.createElement('canvas');
            const size = 64; // Ausgabegröße
            canvas.width = size;
            canvas.height = size;
            const ctx = canvas.getContext('2d');

            // Pixelated rendering für Minecraft-Style
            ctx.imageSmoothingEnabled = false;

            // Gesicht: 8x8 Pixel ab Position (8, 8) in der Skin-Textur
            ctx.drawImage(img, 8, 8, 8, 8, 0, 0, size, size);

            // Helm-Overlay: 8x8 Pixel ab Position (40, 8) – drüberlegen
            ctx.drawImage(img, 40, 8, 8, 8, 0, 0, size, size);

            const headDataUrl = canvas.toDataURL('image/png');

            // Alle Avatar-Elemente sofort aktualisieren
            const accountHead = document.getElementById('account-head');
            if (accountHead) accountHead.src = headDataUrl;

            const currentHead = document.getElementById('current-skin-head');
            if (currentHead) currentHead.src = headDataUrl;

            debugLog('Avatare sofort aus Skin-Textur aktualisiert', 'success');
        } catch (e) {
            debugLog('Fehler beim Rendern des Avatars aus Skin: ' + e, 'warning');
        }
    };
    img.onerror = () => {
        debugLog('Konnte Skin-Bild nicht laden für Avatar-Extraktion', 'warning');
    };
    img.src = skinDataUrl;
}

function updateSkinPage() {
    const playerName = document.getElementById('skin-player-name');
    const currentHead = document.getElementById('current-skin-head');
    const currentName = document.getElementById('current-skin-name');
    const applyBtn = document.getElementById('skin-apply-btn');
    const uploadBtn = document.getElementById('skin-upload-btn');

    // Initialisiere 3D-Viewer nur wenn Seite sichtbar
    const skinsPage = document.getElementById('page-skins');
    if (skinsPage && !skinsPage.classList.contains('hidden')) {
        initSkinViewer();
    }

    if (activeAccount) {
        const uuid = activeAccount.uuid;
        loadSkinToViewer(uuid);
        currentViewingUuid = null; // Eigener Skin
        currentViewingSkinUrl = null;
        if (playerName) playerName.textContent = activeAccount.username;
        if (currentHead) currentHead.src = `https://mc-heads.net/avatar/${uuid}/64`;
        if (currentName) currentName.textContent = activeAccount.username;
        
        // Upload nur für Microsoft-Accounts
        if (uploadBtn) uploadBtn.style.display = activeAccount.is_microsoft ? 'block' : 'none';
        if (applyBtn) applyBtn.style.display = 'none';
    } else {
        if (uploadBtn) uploadBtn.style.display = 'none';
        if (applyBtn) applyBtn.style.display = 'none';
    }

    renderRecentSkins();
    renderSavedSkins();
}

async function searchPlayerSkin() {
    const input = document.getElementById('skin-search-input');
    const playerName = input.value.trim();

    if (!playerName) {
        showToast('Bitte gib einen Spielernamen ein', 'warning', 3000);
        return;
    }

    try {
        // Mojang API über Rust-Backend (CORS-Proxy)
        const [uuid, name] = await invoke('resolve_player_uuid', { username: playerName });

        // 3D Skin anzeigen
        loadSkinToViewer(uuid);
        
        const nameDisplay = document.getElementById('skin-player-name');
        if (nameDisplay) nameDisplay.textContent = name;

        // Track viewing state
        currentViewingUuid = uuid;
        currentViewingSkinUrl = `https://mc-heads.net/skin/${uuid}`;
        
        // "Übernehmen" Button anzeigen wenn nicht eigener Skin
        const applyBtn = document.getElementById('skin-apply-btn');
        if (applyBtn) {
            const isOwnSkin = activeAccount && activeAccount.uuid === uuid;
            if (!isOwnSkin && activeAccount) {
                applyBtn.style.display = 'block';
                if (activeAccount.is_microsoft) {
                    applyBtn.textContent = `✨ Skin von ${name} übernehmen`;
                    applyBtn.disabled = false;
                    applyBtn.style.opacity = '1';
                } else {
                    applyBtn.textContent = `🔒 Skin übernehmen (nur Microsoft)`;
                    applyBtn.disabled = true;
                    applyBtn.style.opacity = '0.5';
                }
            } else {
                applyBtn.style.display = 'none';
            }
        }

        // Zu "Zuletzt angesehen" hinzufügen
        addToRecentSkins(name, uuid);

        showToast(`Skin von ${name} geladen!`, 'success', 2000);
        input.value = '';

    } catch (error) {
        showToast('Spieler nicht gefunden: ' + error, 'error', 3000);
    }
}

// Skin-Datei hochladen
async function handleSkinFileSelect(event) {
    const file = event.target.files[0];
    if (!file) return;

    if (!file.name.endsWith('.png')) {
        showToast('Bitte wähle eine PNG-Datei', 'warning', 3000);
        return;
    }

    if (file.size > 100 * 1024) { // Max 100KB
        showToast('Die Datei ist zu groß (max. 100KB)', 'warning', 3000);
        return;
    }

    if (!activeAccount || !activeAccount.is_microsoft) {
        showToast('Skin-Upload funktioniert nur mit Microsoft-Accounts', 'error', 3000);
        return;
    }

    try {
        showToast('Skin wird hochgeladen...', 'info', 5000);

        // Datei als Base64 lesen
        const base64 = await new Promise((resolve, reject) => {
            const reader = new FileReader();
            reader.onload = () => {
                // "data:image/png;base64," Prefix entfernen
                const result = reader.result.split(',')[1];
                resolve(result);
            };
            reader.onerror = reject;
            reader.readAsDataURL(file);
        });

        const variant = document.querySelector('input[name="skin-variant"]:checked')?.value || 'classic';

        await invoke('upload_skin_file', { skinData: base64, variant: variant });

        showToast('Skin erfolgreich hochgeladen! 🎉', 'success', 3000);

        // Skin-Vorschau mit der hochgeladenen Datei aktualisieren
        const dataUrl = `data:image/png;base64,${base64}`;
        if (skinViewer) {
            skinViewer.loadSkin(dataUrl);
        }
        
        // Skin lokal speichern für "Gespeichert"-Sektion
        if (activeAccount) {
            const skinName = file.name.replace('.png', '') || activeAccount.username;
            addToSavedSkins(skinName, activeAccount.uuid, dataUrl);
        }
        
        const nameDisplay = document.getElementById('skin-player-name');
        if (nameDisplay && activeAccount) nameDisplay.textContent = activeAccount.username;
        
        currentViewingUuid = null;
        currentViewingSkinUrl = null;
        const applyBtn = document.getElementById('skin-apply-btn');
        if (applyBtn) applyBtn.style.display = 'none';

        // Avatare sofort aus der hochgeladenen Skin-Textur aktualisieren
        updateAvatarsFromSkinData(dataUrl);

    } catch (error) {
        showToast('Fehler beim Upload: ' + error, 'error', 5000);
    }

    // File-Input zurücksetzen
    event.target.value = '';
}

// Angezeigten Skin übernehmen
async function applyViewedSkin() {
    if (!currentViewingSkinUrl) {
        showToast('Kein Skin zum Übernehmen ausgewählt', 'warning', 3000);
        return;
    }

    if (!activeAccount || !activeAccount.is_microsoft) {
        showToast('Skin-Änderung funktioniert nur mit Microsoft-Accounts', 'error', 3000);
        return;
    }

    try {
        showToast('Skin wird übernommen...', 'info', 5000);

        const variant = document.querySelector('input[name="skin-variant"]:checked')?.value || 'classic';
        const savedUuid = currentViewingUuid;
        const savedName = document.getElementById('skin-player-name')?.textContent || 'Unknown';
        const savedSkinUrl = currentViewingSkinUrl;
        
        // Prüfe ob es eine Data-URL ist (z.B. von gespeichertem Skin)
        if (currentViewingSkinUrl.startsWith('data:')) {
            // Data-URL: Base64 extrahieren und als Datei-Upload senden
            const b64 = currentViewingSkinUrl.split(',')[1];
            await invoke('upload_skin_file', { skinData: b64, variant: variant });
        } else {
            // Normale URL: über apply_skin_from_url herunterladen und senden
            await invoke('apply_skin_from_url', { skinUrl: currentViewingSkinUrl, variant: variant });
        }

        showToast('Skin erfolgreich übernommen! 🎉', 'success', 3000);

        // Skin lokal speichern für "Gespeichert"-Sektion
        if (savedUuid) {
            try {
                // Wenn wir schon eine Data-URL haben, direkt verwenden
                const skinDataUrl = savedSkinUrl.startsWith('data:') 
                    ? savedSkinUrl 
                    : await invoke('get_skin_texture', { uuid: savedUuid });
                addToSavedSkins(savedName, savedUuid, skinDataUrl);
                // Avatare sofort aus Skin-Textur aktualisieren (kein mc-heads.net Cache!)
                updateAvatarsFromSkinData(skinDataUrl);
            } catch (e) {
                debugLog('Skin konnte nicht gespeichert werden: ' + e, 'warning');
            }
        }

        // Button ausblenden
        const applyBtn = document.getElementById('skin-apply-btn');
        if (applyBtn) applyBtn.style.display = 'none';

        // Skin ist bereits korrekt im Viewer angezeigt – NICHT von mc-heads.net neu laden,
        // da mc-heads.net den alten Skin noch gecacht hat!
        // Nur Name-Label zurücksetzen
        const nameDisplay = document.getElementById('skin-player-name');
        if (nameDisplay && activeAccount) nameDisplay.textContent = activeAccount.username;

        currentViewingUuid = null;
        currentViewingSkinUrl = null;


    } catch (error) {
        showToast('Fehler: ' + error, 'error', 5000);
    }
}

function addToRecentSkins(name, uuid) {
    // Entferne wenn bereits vorhanden
    recentSkins = recentSkins.filter(s => s.uuid !== uuid);

    // Am Anfang hinzufügen (neueste links)
    recentSkins.unshift({ name, uuid });

    // Max 10 Skins (eine Zeile)
    if (recentSkins.length > 10) {
        recentSkins = recentSkins.slice(0, 10);
    }

    localStorage.setItem('recentSkins', JSON.stringify(recentSkins));
    renderRecentSkins();
}

function renderRecentSkins() {
    const container = document.getElementById('recent-skins');
    if (!container) return;

    if (recentSkins.length === 0) {
        container.innerHTML = '<p style="color: var(--text-secondary); font-size: 12px;">Noch keine Skins angesehen</p>';
        return;
    }

    const hasMsAccount = activeAccount && activeAccount.is_microsoft;

    container.innerHTML = recentSkins.map(skin => {
        const isOwnSkin = activeAccount && activeAccount.uuid === skin.uuid;
        const showApply = hasMsAccount && !isOwnSkin;
        return `
        <div class="skin-thumbnail" onclick="showSkin('${skin.uuid}', '${skin.name}')">
            <img src="https://mc-heads.net/avatar/${skin.uuid}/48" alt="${skin.name}">
            <p class="skin-name">${skin.name}</p>
            ${showApply ? `<button class="skin-action-btn" onclick="event.stopPropagation(); quickApplySkin('${skin.uuid}', '${skin.name}')" 
                title="Skin übernehmen">✨</button>` : ''}
        </div>`;
    }).join('');
}

// ==================== GESPEICHERTE SKINS ====================

async function addToSavedSkins(name, uuid, skinDataUrl) {
    // Prüfe ob schon vorhanden
    const existing = savedSkins.find(s => s.uuid === uuid && s.name === name);
    if (existing) return; // Nicht doppelt speichern
    
    try {
        // Skin lokal über Rust speichern
        const filename = await invoke('save_skin_locally', { uuid, name, skinData: skinDataUrl });
        
        // Am Anfang hinzufügen (neueste zuerst)
        savedSkins.unshift({ 
            name, 
            uuid, 
            filename,
            timestamp: Date.now() 
        });
        
        localStorage.setItem('savedSkins', JSON.stringify(savedSkins));
        renderSavedSkins();
        debugLog(`Skin "${name}" lokal gespeichert`, 'success');
    } catch (e) {
        debugLog('Fehler beim Speichern des Skins: ' + e, 'error');
    }
}

function renderSavedSkins() {
    const container = document.getElementById('saved-skins');
    if (!container) return;

    if (savedSkins.length === 0) {
        container.innerHTML = '<p style="color: var(--text-secondary); font-size: 12px;">Noch keine Skins gespeichert – Equippe einen Skin um ihn hier zu speichern</p>';
        return;
    }

    const hasMsAccount = activeAccount && activeAccount.is_microsoft;

    container.innerHTML = savedSkins.map((skin, index) => `
        <div class="skin-thumbnail">
            <div onclick="showSavedSkin('${skin.uuid}', '${skin.name}', '${skin.filename || ''}')">
                <img src="https://mc-heads.net/avatar/${skin.uuid}/48" alt="${skin.name}">
                <p class="skin-name">${skin.name}</p>
            </div>
            ${hasMsAccount ? `<button class="skin-action-btn" onclick="event.stopPropagation(); quickApplySavedSkin(${index})"
                title="Skin equippen">✨</button>` : ''}
            <button class="skin-action-btn skin-delete-btn" onclick="event.stopPropagation(); removeSavedSkin(${index})"
                title="Aus Gespeichert entfernen" style="top: -4px; left: -4px; right: auto;">🗑</button>
        </div>`).join('');
}

async function showSavedSkin(uuid, name, filename) {
    // Versuche Skin vom lokalen Speicher zu laden
    if (filename) {
        try {
            const skinDataUrl = await invoke('load_saved_skin', { filename });
            if (skinViewer) {
                if (!skinViewer) initSkinViewer();
                skinViewer.loadSkin(skinDataUrl);
            }
            const nameDisplay = document.getElementById('skin-player-name');
            if (nameDisplay) nameDisplay.textContent = name;
            
            currentViewingUuid = uuid;
            currentViewingSkinUrl = skinDataUrl; // Lokale Data-URL verwenden
            
            // "Übernehmen" Button anzeigen
            const applyBtn = document.getElementById('skin-apply-btn');
            if (applyBtn && activeAccount) {
                const isOwnSkin = activeAccount.uuid === uuid;
                if (!isOwnSkin && activeAccount.is_microsoft) {
                    applyBtn.style.display = 'block';
                    applyBtn.textContent = `✨ Skin von ${name} übernehmen`;
                    applyBtn.disabled = false;
                    applyBtn.style.opacity = '1';
                } else {
                    applyBtn.style.display = 'none';
                }
            }
            return;
        } catch (e) {
            debugLog('Lokaler Skin konnte nicht geladen werden, fallback: ' + e, 'warning');
        }
    }
    // Fallback: normal anzeigen
    showSkin(uuid, name);
}

async function quickApplySavedSkin(index) {
    const skin = savedSkins[index];
    if (!skin) return;
    
    if (!activeAccount || !activeAccount.is_microsoft) {
        showToast('Skin-Änderung funktioniert nur mit Microsoft-Accounts', 'error', 3000);
        return;
    }
    
    try {
        // Lokalen Skin laden und als Datei hochladen
        let skinData = null;
        if (skin.filename) {
            try {
                skinData = await invoke('load_saved_skin', { filename: skin.filename });
            } catch (e) {
                debugLog('Lokaler Skin nicht gefunden, lade von Server: ' + e, 'warning');
            }
        }
        
        showToast(`Skin "${skin.name}" wird übernommen...`, 'info', 5000);
        const variant = document.querySelector('input[name="skin-variant"]:checked')?.value || 'classic';
        
        if (skinData) {
            // Base64 Daten extrahieren und als Datei-Upload senden
            const b64 = skinData.includes(',') ? skinData.split(',')[1] : skinData;
            await invoke('upload_skin_file', { skinData: b64, variant });
        } else {
            // Fallback: von mc-heads.net laden
            const skinUrl = `https://mc-heads.net/skin/${skin.uuid}`;
            await invoke('apply_skin_from_url', { skinUrl, variant });
        }
        
        showToast(`Skin "${skin.name}" erfolgreich übernommen! 🎉`, 'success', 3000);
        
        // Viewer aktualisieren
        if (skinData && skinViewer) {
            skinViewer.loadSkin(skinData);
        } else {
            loadSkinToViewer(skin.uuid);
        }
        
        // Avatare sofort aus Skin-Textur aktualisieren
        if (skinData) {
            updateAvatarsFromSkinData(skinData);
        } else {
            // Fallback: Skin-Textur vom Server laden und Avatar daraus rendern
            try {
                const skinDataUrl = await invoke('get_skin_texture', { uuid: skin.uuid });
                updateAvatarsFromSkinData(skinDataUrl);
            } catch (e) {
                debugLog('Avatar-Update fehlgeschlagen: ' + e, 'warning');
            }
        }
        
        const nameDisplay = document.getElementById('skin-player-name');
        if (nameDisplay && activeAccount) nameDisplay.textContent = activeAccount.username;
        
        currentViewingUuid = null;
        currentViewingSkinUrl = null;
        const applyBtn = document.getElementById('skin-apply-btn');
        if (applyBtn) applyBtn.style.display = 'none';
        
    } catch (error) {
        showToast('Fehler: ' + error, 'error', 5000);
    }
}

async function removeSavedSkin(index) {
    const skin = savedSkins[index];
    if (!skin) return;
    
    // Lokale Datei löschen
    if (skin.filename) {
        try {
            await invoke('delete_saved_skin', { filename: skin.filename });
        } catch (e) {
            debugLog('Konnte lokale Skin-Datei nicht löschen: ' + e, 'warning');
        }
    }
    
    savedSkins.splice(index, 1);
    localStorage.setItem('savedSkins', JSON.stringify(savedSkins));
    renderSavedSkins();
    showToast(`Skin "${skin.name}" entfernt`, 'info', 2000);
}

function showSkin(uuid, name) {
    loadSkinToViewer(uuid);
    const nameDisplay = document.getElementById('skin-player-name');
    if (nameDisplay) nameDisplay.textContent = name;
    
    currentViewingUuid = uuid;
    currentViewingSkinUrl = `https://mc-heads.net/skin/${uuid}`;
    
    // "Übernehmen" Button anzeigen
    const applyBtn = document.getElementById('skin-apply-btn');
    if (applyBtn) {
        const isOwnSkin = activeAccount && activeAccount.uuid === uuid;
        if (!isOwnSkin && activeAccount) {
            applyBtn.style.display = 'block';
            if (activeAccount.is_microsoft) {
                applyBtn.textContent = `✨ Skin von ${name} übernehmen`;
                applyBtn.disabled = false;
                applyBtn.style.opacity = '1';
            } else {
                applyBtn.textContent = `🔒 Skin übernehmen (nur Microsoft)`;
                applyBtn.disabled = true;
                applyBtn.style.opacity = '0.5';
            }
        } else {
            applyBtn.style.display = 'none';
        }
    }
}

// Schnell-Skin-Übernahme direkt aus der "Zuletzt angesehen"-Liste
async function quickApplySkin(uuid, name) {
    if (!activeAccount || !activeAccount.is_microsoft) {
        showToast('Skin-Änderung funktioniert nur mit Microsoft-Accounts', 'error', 3000);
        return;
    }
    
    const skinUrl = `https://mc-heads.net/skin/${uuid}`;
    try {
        showToast(`Skin von ${name} wird übernommen...`, 'info', 5000);
        const variant = document.querySelector('input[name="skin-variant"]:checked')?.value || 'classic';
        await invoke('apply_skin_from_url', { skinUrl: skinUrl, variant: variant });
        showToast(`Skin von ${name} erfolgreich übernommen! 🎉`, 'success', 3000);
        
        // Skin lokal speichern für "Gespeichert"-Sektion
        try {
            const skinDataUrl = await invoke('get_skin_texture', { uuid: uuid });
            addToSavedSkins(name, uuid, skinDataUrl);
            // Avatare sofort aus Skin-Textur aktualisieren (kein mc-heads.net Cache!)
            updateAvatarsFromSkinData(skinDataUrl);
        } catch (e) {
            debugLog('Skin konnte nicht gespeichert werden: ' + e, 'warning');
        }
        
        // Skin des Quell-Spielers im Viewer anzeigen (das ist jetzt unser neuer Skin)
        // NICHT von mc-heads.net/activeAccount.uuid laden – dort ist noch der alte Skin gecacht!
        loadSkinToViewer(uuid);
        
        const nameDisplay = document.getElementById('skin-player-name');
        if (nameDisplay && activeAccount) nameDisplay.textContent = activeAccount.username;
        
        currentViewingUuid = null;
        currentViewingSkinUrl = null;
        const applyBtn = document.getElementById('skin-apply-btn');
        if (applyBtn) applyBtn.style.display = 'none';
        
    } catch (error) {
        showToast('Fehler: ' + error, 'error', 5000);
    }
}

// Helpers
function formatNumber(num) {
    if (num >= 1000000) {
        return (num / 1000000).toFixed(1) + 'M';
    }
    if (num >= 1000) {
        return (num / 1000).toFixed(1) + 'K';
    }
    return num.toString();
}

// ==================== MOD DETAILS ====================

let currentModDetails = null;

function handleModCardClick(event, modId, source) {
    // Prüfe ob der Click auf einem Button oder Link war
    const target = event.target;
    const isButton = target.tagName === 'BUTTON' || target.closest('button');
    const isLink = target.tagName === 'A' || target.closest('a');

    if (isButton || isLink) {
        // Button/Link wurde geklickt - nicht zur Details-Seite navigieren
        event.stopPropagation();
        return;
    }

    // Ansonsten öffne Mod-Details
    showModDetails(modId, source);
}

// Funktion zum Öffnen von Mod-Details vom Profile Content aus
async function showModDetailsFromProfile(modId, source = 'modrinth') {
    await showModDetails(modId, source);
}

async function showModDetails(modId, source = 'modrinth') {
    debugLog(`Opening mod details for ${modId} from ${source}`);
    recordNavigation({
        type: 'mod',
        modId,
        source,
        serverId: serverBrowserContext?.serverId || null,
        profileId: !serverBrowserContext && currentProfile ? currentProfile.id : null
    });

    // Setze Tab zurück auf Description und Filter zurück
    currentModDetailsTab = 'description';

    // Initialisiere Filter mit Profil-/Server-Werten wenn vorhanden
    if (serverBrowserContext) {
        modDetailsVersionFilter = {
            loader: '',
            mcVersion: serverBrowserContext.minecraftVersion || '',
            includeSnapshots: false,
        };
    } else if (currentProfile) {
        debugLog(`Current Profile: ${currentProfile.name}, MC: ${currentProfile.minecraft_version}, Loader: ${currentProfile.loader?.loader}`, 'info');

        modDetailsVersionFilter.mcVersion = currentProfile.minecraft_version || '';

        // Loader-Name direkt aus Profil (wie im Content Browser)
        const loaderName = currentProfile.loader?.loader;
        if (loaderName && loaderName !== 'vanilla') {
            modDetailsVersionFilter.loader = loaderName.toLowerCase();
        } else {
            modDetailsVersionFilter.loader = '';
        }

        modDetailsVersionFilter.includeSnapshots = false;
        debugLog(`Initialized version filter - MC: ${modDetailsVersionFilter.mcVersion}, Loader: ${modDetailsVersionFilter.loader}`, 'info');
    } else {
        modDetailsVersionFilter = { loader: '', mcVersion: '', includeSnapshots: false };
    }

    // Wechsle zur Details-Seite
    switchPage('mod-details');

    const content = document.getElementById('mod-details-content');
    if (!content) return;

    content.innerHTML = '<div class="loading"><div class="spinner" style="margin: 20px auto;"></div><p>Loading mod details...</p></div>';

    try {
        // Lade Mod-Details von der API
        const mod = await invoke('get_mod_info', { modId, source });
        const versions = await invoke('get_mod_versions', { modId, source });

        currentModDetails = { mod, versions, source };

        await renderModDetails(mod, versions);
    } catch (error) {
        debugLog('Failed to load mod details: ' + error, 'error');
        content.innerHTML = `<div class="loading">Failed to load mod details: ${error}</div>`;
    }
}

// Track current tab and filter state for mod details
let currentModDetailsTab = 'description';
let modDetailsVersionFilter = { loader: '', mcVersion: '', includeSnapshots: false };

async function renderModDetails(mod, versions) {
    const content = document.getElementById('mod-details-content');
    const nameHeader = document.getElementById('mod-details-name');

    if (nameHeader) nameHeader.textContent = mod.name;

    // Environment Labels
    const clientSide = mod.client_side;
    const serverSide = mod.server_side;
    let envLabel = '';

    if (clientSide === 'required' && serverSide === 'required') {
        envLabel = 'Client & Server';
    } else if ((clientSide === 'optional' && serverSide === 'optional') ||
        (clientSide === 'required' && serverSide === 'optional') ||
        (clientSide === 'optional' && serverSide === 'required')) {
        envLabel = 'Client or Server';
    } else if ((clientSide === 'required' || clientSide === 'optional') &&
        (serverSide === 'unsupported' || serverSide === 'unknown' || !serverSide)) {
        envLabel = 'Client';
    } else if ((serverSide === 'required' || serverSide === 'optional') &&
        (clientSide === 'unsupported' || clientSide === 'unknown' || !clientSide)) {
        envLabel = 'Server';
    }

    // Sammle alle einzigartigen Loader und MC-Versionen
    const allLoaders = new Set();
    const allMcVersions = new Set();
    versions.forEach(v => {
        if (v.loaders) v.loaders.forEach(l => allLoaders.add(l.toLowerCase()));
        // Nur Release-Versionen zu den Filtern hinzufügen
        const versionType = (v.version_type || 'release').toLowerCase();
        if (versionType === 'release' && v.game_versions) {
            v.game_versions.forEach(gv => {
                // Zusätzlicher Check: Filteriere Snapshot-ähnliche Versionen
                if (!gv.toLowerCase().includes('snapshot') &&
                    !gv.toLowerCase().includes('pre') &&
                    !gv.toLowerCase().includes('rc') &&
                    !gv.toLowerCase().includes('alpha') &&
                    !gv.toLowerCase().includes('beta')) {
                    allMcVersions.add(gv);
                }
            });
        }
    });

    // Sortiere MC-Versionen (neueste zuerst)
    const sortedMcVersions = Array.from(allMcVersions).sort((a, b) => {
        const partsA = a.split('.').map(n => parseInt(n) || 0);
        const partsB = b.split('.').map(n => parseInt(n) || 0);
        for (let i = 0; i < Math.max(partsA.length, partsB.length); i++) {
            const diff = (partsB[i] || 0) - (partsA[i] || 0);
            if (diff !== 0) return diff;
        }
        return 0;
    });

    content.innerHTML = `
        <!-- Header Section -->
        <div style="display: grid; grid-template-columns: 80px 1fr auto; gap: 20px; margin-bottom: 20px; background: var(--bg-medium); padding: 20px; border-radius: 12px;">
            <!-- Mod Icon -->
            <div style="width: 80px; height: 80px; flex-shrink: 0;">
                ${mod.icon_url ? 
                    `<img src="${escapeAttr(mod.icon_url)}" alt="${escapeAttr(mod.name)}" 
                         style="width: 100%; height: 100%; object-fit: cover; border-radius: 8px;"
                         data-fallback="icon-large">` :
                    `<div style="font-size: 48px; display: flex; align-items: center; justify-content: center; width: 100%; height: 100%; background: var(--bg-dark); border-radius: 8px;"><i class="bi bi-box"></i></div>`
                }
            </div>
            
            <!-- Mod Info -->
            <div>
                <h2 style="margin: 0 0 6px 0; font-size: 24px; color: var(--text-primary);">${escapeHtml(mod.name)}</h2>
                <div style="display: flex; align-items: center; gap: 10px; margin-bottom: 10px;">
                    <span style="color: var(--text-secondary); font-size: 13px;">by</span>
                    <a href="https://modrinth.com/user/${encodeURIComponent(mod.author || '')}"
                       target="_blank"
                       style="color: var(--gold); font-size: 13px; text-decoration: underline;">
                        ${escapeHtml(mod.author)}
                    </a>
                </div>
                <div style="display: flex; flex-wrap: wrap; gap: 6px; align-items: center; margin-bottom: 12px;">
                    ${envLabel ? `<span style="background: var(--bg-dark); color: var(--gold); font-size: 11px; padding: 3px 8px; border-radius: 4px; font-weight: 600; border: 1px solid var(--gold);">${envLabel}</span>` : ''}
                    ${mod.categories && mod.categories.length > 0 ? mod.categories.slice(0, 5).map(cat =>
                        `<span style="background: var(--bg-dark); color: var(--text-secondary); font-size: 10px; padding: 3px 8px; border-radius: 4px;">${escapeHtml(cat)}</span>`
                    ).join('') : ''}
                </div>
                <p style="color: var(--text-secondary); font-size: 14px; margin: 0; line-height: 1.6;">
                    ${escapeHtml(mod.description)}
                </p>
            </div>
            
            <!-- Right Side: Install Button, Last Updated, Downloads -->
            <div style="display: flex; flex-direction: column; align-items: flex-end; gap: 12px; min-width: 200px;">
                ${canInstallFromDetails() ?
                    `<button id="mod-details-install-btn" class="btn btn-primary" onclick="installModFromDetails('${escapeAttr(mod.id)}', this)"
                             style="width: 100%; padding: 14px 24px; font-size: 15px; font-weight: 600;">
                        <i class="bi bi-download"></i> Install
                    </button>` :
                    `<button class="btn btn-secondary" disabled
                             style="width: 100%; padding: 14px 24px; font-size: 15px; opacity: 0.5; cursor: not-allowed;">
                        Create a profile first
                    </button>`
                }
                
                <div style="text-align: right; padding: 8px 0;">
                    <div style="color: var(--text-secondary); font-size: 11px; margin-bottom: 4px;">Last Updated</div>
                    <div style="color: var(--text-primary); font-size: 13px;">${new Date(mod.updated_at).toLocaleDateString()}</div>
                </div>
                
                <div style="text-align: right; padding: 8px 0;">
                    <div style="color: var(--text-primary); font-size: 24px; font-weight: 700;">${formatNumber(mod.downloads)}</div>
                    <div style="color: var(--text-secondary); font-size: 12px;">downloads</div>
                </div>
            </div>
        </div>
        
        <!-- Tabs Navigation -->
        <div style="display: flex; gap: 5px; margin-bottom: 15px; background: var(--bg-medium); padding: 6px; border-radius: 10px;">
            <button class="mod-details-tab ${currentModDetailsTab === 'description' ? 'active' : ''}" 
                    onclick="switchModDetailsTab('description')"
                    style="flex: 1; padding: 12px 20px; border: none; border-radius: 8px; cursor: pointer; font-size: 14px; font-weight: 600;
                           background: ${currentModDetailsTab === 'description' ? 'var(--gold)' : 'transparent'};
                           color: ${currentModDetailsTab === 'description' ? 'var(--bg-dark)' : 'var(--text-secondary)'};">
                <i class="bi bi-file-text"></i> Description
            </button>
            <button class="mod-details-tab ${currentModDetailsTab === 'versions' ? 'active' : ''}" 
                    onclick="switchModDetailsTab('versions')"
                    style="flex: 1; padding: 12px 20px; border: none; border-radius: 8px; cursor: pointer; font-size: 14px; font-weight: 600;
                           background: ${currentModDetailsTab === 'versions' ? 'var(--gold)' : 'transparent'};
                           color: ${currentModDetailsTab === 'versions' ? 'var(--bg-dark)' : 'var(--text-secondary)'};">
                <i class="bi bi-box"></i> Versions (${versions.length})
            </button>
            <button class="mod-details-tab ${currentModDetailsTab === 'gallery' ? 'active' : ''}" 
                    onclick="switchModDetailsTab('gallery')"
                    style="flex: 1; padding: 12px 20px; border: none; border-radius: 8px; cursor: pointer; font-size: 14px; font-weight: 600;
                           background: ${currentModDetailsTab === 'gallery' ? 'var(--gold)' : 'transparent'};
                           color: ${currentModDetailsTab === 'gallery' ? 'var(--bg-dark)' : 'var(--text-secondary)'};">
                <i class="bi bi-images"></i> Gallery
            </button>
        </div>
        
        <!-- Tab Content -->
        <div id="mod-details-tab-content">
            <div class="loading"><div class="spinner" style="margin: 20px auto;"></div></div>
        </div>
    `;

    // Install-Button als "Installed" markieren, falls schon im Profil/Server vorhanden
    updateDetailsInstallButtonState(mod);

    // Rendere Tab-Content separat (async)
    document.getElementById('mod-details-tab-content').innerHTML = await renderModDetailsTabContent(mod, versions, allLoaders, sortedMcVersions);
}

async function renderModDetailsTabContent(mod, versions, allLoaders, sortedMcVersions) {
    switch (currentModDetailsTab) {
        case 'description':
            return renderDescriptionTab(mod);
        case 'versions':
            return await renderVersionsTab(mod, versions, allLoaders, sortedMcVersions);
        case 'gallery':
            return renderGalleryTab(mod);
        default:
            return renderDescriptionTab(mod);
    }
}

function renderDescriptionTab(mod) {
    // Body/Long Description - falls verfügbar, sonst kurze Beschreibung
    const longDescription = mod.body || mod.description || 'No description available.';

    // Konvertiere Markdown zu einfachem HTML (basic)
    const formattedDescription = formatMarkdown(longDescription);

    return `
        <div style="background: var(--bg-medium); padding: 24px; border-radius: 12px; margin-bottom: 20px;">
            <h3 style="margin: 0 0 16px 0; font-size: 18px; color: var(--text-primary);">About this project</h3>
            <div class="project-body">
                ${formattedDescription}
            </div>
        </div>
        
        <!-- Links Section -->
        <div style="background: var(--bg-medium); padding: 20px; border-radius: 12px;">
            <h3 style="margin: 0 0 12px 0; font-size: 16px; color: var(--text-primary);">External Resources</h3>
            <div style="display: flex; flex-wrap: wrap; gap: 10px;">
                <a href="https://modrinth.com/mod/${mod.slug || mod.id}" target="_blank" 
                   class="btn btn-secondary" style="padding: 10px 16px; text-decoration: none; font-size: 13px;">
                    <i class="bi bi-globe"></i> Modrinth
                </a>
                ${mod.source_url ? 
                    `<a href="${mod.source_url}" target="_blank" 
                        class="btn btn-secondary" style="padding: 10px 16px; text-decoration: none; font-size: 13px;">
                        <i class="bi bi-code-slash"></i> Source Code
                    </a>` : ''
                }
                ${mod.issues_url ? 
                    `<a href="${mod.issues_url}" target="_blank" 
                        class="btn btn-secondary" style="padding: 10px 16px; text-decoration: none; font-size: 13px;">
                        <i class="bi bi-bug"></i> Issues
                    </a>` : ''
                }
                ${mod.wiki_url ? 
                    `<a href="${mod.wiki_url}" target="_blank" 
                        class="btn btn-secondary" style="padding: 10px 16px; text-decoration: none; font-size: 13px;">
                        <i class="bi bi-book"></i> Wiki
                    </a>` : ''
                }
                ${mod.discord_url ? 
                    `<a href="${mod.discord_url}" target="_blank" 
                        class="btn btn-secondary" style="padding: 10px 16px; text-decoration: none; font-size: 13px;">
                        <i class="bi bi-chat-dots"></i> Discord
                    </a>` : ''
                }
            </div>
        </div>
    `;
}

async function renderVersionsTab(mod, versions, allLoaders, sortedMcVersions) {
    // Installierte Version im aktuellen Profil/Server ermitteln (Version-ID bevorzugt, sonst Versionsnummer)
    let installedVersion = null;
    let installedVersionId = null;
    const detailsKind = getDetailsContentKind(mod);

    if (serverBrowserContext) {
        await loadServerInstalledProjectIds(true);
        const installedItem = serverInstalledContentCache.find(item => item.project_id === mod.id);
        if (installedItem && installedItem.version) {
            installedVersion = installedItem.version;
        }
    } else if (currentProfile) {
        try {
            if (detailsKind === 'resourcepacks' || detailsKind === 'shaderpacks') {
                const packs = await invoke(detailsKind === 'resourcepacks' ? 'get_installed_resourcepacks' : 'get_installed_shaderpacks', { profileId: currentProfile.id });
                const installedPack = (packs || []).find(p => p.project_id === mod.id);
                installedVersionId = installedPack?.version_id || null;
            } else {
                const installedMods = await invoke('get_installed_mods', { profileId: currentProfile.id });
                const installedMod = (installedMods || []).find(m => m.mod_id === mod.id || (mod.slug && m.mod_id === mod.slug));
                installedVersion = installedMod?.version || null;
                installedVersionId = installedMod?.version_id || null;
            }
        } catch (error) {
            debugLog('Failed to get installed content: ' + error, 'error');
        }
    }
    const canInstall = canInstallFromDetails();

    // Filtere Versionen basierend auf Filter
    let filteredVersions = versions;

    if (modDetailsVersionFilter.loader) {
        filteredVersions = filteredVersions.filter(v => {
            if (!v.loaders) return false;

            const selectedLoader = modDetailsVersionFilter.loader.toLowerCase();

            // Wenn Quilt ausgewählt ist, akzeptiere auch Fabric-Mods
            if (selectedLoader === 'quilt') {
                return v.loaders.some(l => {
                    const loader = l.toLowerCase();
                    return loader === 'quilt' || loader === 'fabric';
                });
            }

            // Sonst normaler Vergleich
            return v.loaders.some(l => l.toLowerCase() === selectedLoader);
        });
    }

    if (modDetailsVersionFilter.mcVersion) {
        filteredVersions = filteredVersions.filter(v =>
            v.game_versions && v.game_versions.includes(modDetailsVersionFilter.mcVersion)
        );
    }

    // Filtere Snapshots aus wenn nicht aktiviert
    if (!modDetailsVersionFilter.includeSnapshots) {
        filteredVersions = filteredVersions.filter(v => {
            const versionType = (v.version_type || 'release').toLowerCase();
            return versionType === 'release';
        });
    }

    // Sortiere Versionen nach Datum (neueste zuerst)
    filteredVersions.sort((a, b) => new Date(b.published) - new Date(a.published));

    debugLog(`Creating loader options. Available loaders: ${Array.from(allLoaders).join(', ')}, Selected: ${modDetailsVersionFilter.loader}`, 'info');

    const loaderOptions = Array.from(allLoaders).map(l =>
        `<option value="${l}" ${modDetailsVersionFilter.loader === l ? 'selected' : ''}>${l.charAt(0).toUpperCase() + l.slice(1)}</option>`
    ).join('');

    const mcVersionOptions = sortedMcVersions.map(v =>
        `<option value="${v}" ${modDetailsVersionFilter.mcVersion === v ? 'selected' : ''}>${v}</option>`
    ).join('');

    return `
        <div style="background: var(--bg-medium); padding: 20px; border-radius: 12px;">
            <!-- Filter Bar -->
            <div style="display: flex; gap: 15px; margin-bottom: 20px; flex-wrap: wrap; align-items: center;">
                <div style="display: flex; align-items: center; gap: 8px;">
                    <label style="color: var(--text-secondary); font-size: 13px; white-space: nowrap;">Loader:</label>
                    <select id="version-filter-loader" onchange="filterModVersions()"
                            style="padding: 8px 12px; background: var(--bg-dark); border: 1px solid var(--bg-light); border-radius: 6px; color: var(--text-primary); font-size: 13px; min-width: 120px;">
                        <option value="">All Loaders</option>
                        ${loaderOptions}
                    </select>
                </div>
                <div style="display: flex; align-items: center; gap: 8px;">
                    <label style="color: var(--text-secondary); font-size: 13px; white-space: nowrap;">MC Version:</label>
                    <select id="version-filter-mc" onchange="filterModVersions()"
                            style="padding: 8px 12px; background: var(--bg-dark); border: 1px solid var(--bg-light); border-radius: 6px; color: var(--text-primary); font-size: 13px; min-width: 100px;">
                        <option value="">All Versions</option>
                        ${mcVersionOptions}
                    </select>
                </div>
                <button class="btn btn-secondary" 
                        onclick="toggleSnapshotFilter()"
                        style="padding: 8px 16px; font-size: 12px; display: flex; align-items: center; gap: 6px; ${modDetailsVersionFilter.includeSnapshots ? 'background: var(--gold); color: var(--bg-dark);' : ''}">
                    ${modDetailsVersionFilter.includeSnapshots ? '<i class="bi bi-check"></i>' : ''} Snapshots
                </button>
                <div style="flex: 1;"></div>
                <span style="color: var(--text-secondary); font-size: 12px;">
                    ${filteredVersions.length} version${filteredVersions.length !== 1 ? 's' : ''} found
                </span>
            </div>
            
            <!-- Versions List -->
            ${filteredVersions.length === 0 ? 
                `<p style="color: var(--text-secondary); padding: 40px; text-align: center;">
                    No versions match your filter criteria.
                </p>` :
                `<div style="max-height: 500px; overflow-y: auto;">
                    ${filteredVersions.map(version => {
                        const versionType = version.version_type || 'release';
                        const typeColor = versionType === 'release' ? '#4caf50' : 
                                         versionType === 'beta' ? '#ff9800' : '#f44336';
                        const typeLabel = versionType.charAt(0).toUpperCase() + versionType.slice(1);
                        
                        // Prüfe ob diese Version installiert ist
                        const isInstalled = installedVersionId
                            ? installedVersionId === version.id
                            : (!!installedVersion && installedVersion === version.version_number);
                        
                        return `
                        <div style="background: var(--bg-dark); padding: 16px; border-radius: 8px; margin-bottom: 10px; border-left: 3px solid ${typeColor};">
                            <div style="display: flex; justify-content: space-between; align-items: flex-start; margin-bottom: 10px;">
                                <div style="flex: 1;">
                                    <div style="display: flex; align-items: center; gap: 10px; margin-bottom: 4px; flex-wrap: wrap;">
                                        <span style="font-weight: 600; color: var(--text-primary); font-size: 15px;">${version.name || version.version_number}</span>
                                        <span style="background: ${typeColor}; color: white; font-size: 10px; padding: 2px 6px; border-radius: 3px; font-weight: 600;">${typeLabel}</span>
                                        ${isInstalled ? 
                                            `<span style="background: var(--gold); color: var(--bg-dark); font-size: 10px; padding: 3px 8px; border-radius: 3px; font-weight: 700;"><i class="bi bi-check"></i> INSTALLED</span>` 
                                            : ''
                                        }
                                    </div>
                                    <span style="color: var(--text-secondary); font-size: 12px;">${version.version_number}</span>
                                </div>
                                ${canInstall ?
                                    (isInstalled ?
                                        `<button class="btn btn-secondary" disabled style="padding: 8px 20px; font-size: 13px; opacity: 0.6;">
                                            <i class="bi bi-check"></i> Installed
                                        </button>` :
                                        `<button class="btn btn-primary" onclick="installModVersion('${escapeAttr(mod.id)}', '${escapeAttr(version.id)}', this)"
                                                 style="padding: 8px 20px; font-size: 13px;">
                                            Install
                                        </button>`
                                    ) :
                                    `<button class="btn btn-secondary" disabled style="padding: 8px 20px; font-size: 13px; opacity: 0.5; cursor: not-allowed;">
                                        Create a profile first
                                    </button>`
                                }
                            </div>
                            <div style="display: flex; flex-wrap: wrap; gap: 6px; font-size: 11px; margin-bottom: 8px;">
                                ${version.loaders ? version.loaders.map(loader => {
                                    const loaderColors = {
                                        'fabric': '#DBB98F',
                                        'forge': '#3E4758',
                                        'neoforge': '#F28500',
                                        'quilt': '#9B59B6'
                                    };
                                    const bgColor = loaderColors[loader.toLowerCase()] || 'var(--gold)';
                                    return `<span style="background: ${bgColor}; color: ${loader.toLowerCase() === 'forge' ? 'white' : 'var(--bg-dark)'}; padding: 3px 8px; border-radius: 4px; font-weight: 600;">${loader}</span>`;
                                }).join('') : ''}
                            </div>
                            <div style="display: flex; flex-wrap: wrap; gap: 4px; font-size: 10px; margin-bottom: 8px;">
                                ${version.game_versions ? version.game_versions.slice(0, 8).map(gv => 
                                    `<span style="background: var(--bg-medium); color: var(--text-secondary); padding: 2px 6px; border-radius: 3px;">${gv}</span>`
                                ).join('') : ''}
                                ${version.game_versions && version.game_versions.length > 8 ? 
                                    `<span style="color: var(--text-secondary);">+${version.game_versions.length - 8} more</span>` : ''
                                }
                            </div>
                            <div style="display: flex; gap: 15px; color: var(--text-secondary); font-size: 11px;">
                                <span><i class="bi bi-calendar3"></i> ${new Date(version.published).toLocaleDateString()}</span>
                                ${version.downloads ? `<span><i class="bi bi-download"></i> ${formatNumber(version.downloads)}</span>` : ''}
                            </div>
                        </div>
                    `}).join('')}
                </div>`
            }
        </div>
    `;
}

function renderGalleryTab(mod) {
    // Gallery/Screenshots - falls verfügbar
    const gallery = mod.gallery || [];

    if (gallery.length === 0) {
        return `
            <div style="background: var(--bg-medium); padding: 60px 24px; border-radius: 12px; text-align: center;">
                <div style="font-size: 48px; margin-bottom: 15px;"><i class="bi bi-images"></i></div>
                <h3 style="margin: 0 0 10px 0; font-size: 18px; color: var(--text-primary);">No Images Available</h3>
                <p style="color: var(--text-secondary); font-size: 14px;">
                    This project doesn't have any gallery images yet.
                </p>
            </div>
        `;
    }

    return `
        <div style="background: var(--bg-medium); padding: 20px; border-radius: 12px;">
            <div style="display: grid; grid-template-columns: repeat(auto-fill, minmax(280px, 1fr)); gap: 15px;">
                ${gallery.map((img, index) => {
                    const imageUrl = normalizeMediaUrl(img.url || img);
                    // raw_url = Originalgröße; url ist nur eine komprimierte Vorschau (~350px)
                    const fullUrl = normalizeMediaUrl(img.raw_url || img.url || img);
                    const safeUrl = escapeAttr(imageUrl);
                    const title = img.title ? escapeHtml(img.title) : '';
                    const alt = escapeAttr(img.title || `Screenshot ${index + 1}`);

                    return `
                        <div style="border-radius: 8px; overflow: hidden; cursor: pointer; position: relative;"
                             data-full-url="${escapeAttr(fullUrl)}"
                             data-preview-url="${safeUrl}"
                             onclick="openGalleryImage(this.dataset.fullUrl, ${index}, this.dataset.previewUrl)">
                            <img src="${safeUrl}" alt="${alt}" loading="lazy" referrerpolicy="no-referrer"
                                 style="width: 100%; height: 180px; object-fit: cover; display: block; transition: transform 0.3s;"
                                 onmouseover="this.style.transform='scale(1.05)'"
                                 onmouseout="this.style.transform='scale(1)'"
                                 data-fallback="gallery">
                            ${title ? `
                                <div style="position: absolute; bottom: 0; left: 0; right: 0; background: linear-gradient(transparent, rgba(0,0,0,0.8)); padding: 10px;">
                                    <span style="color: white; font-size: 12px;">${title}</span>
                                </div>
                            ` : ''}
                        </div>
                    `;
                }).join('')}
            </div>
        </div>
    `;
}

async function switchModDetailsTab(tab) {
    currentModDetailsTab = tab;

    // Re-render nur den Tab-Content, nicht die ganze Seite
    if (currentModDetails) {
        const { mod, versions } = currentModDetails;

        // Sammle alle einzigartigen Loader und MC-Versionen
        const allLoaders = new Set();
        const allMcVersions = new Set();
        versions.forEach(v => {
            if (v.loaders) v.loaders.forEach(l => allLoaders.add(l.toLowerCase()));
            if (v.game_versions) v.game_versions.forEach(gv => allMcVersions.add(gv));
        });

        // Sortiere MC-Versionen
        const sortedMcVersions = Array.from(allMcVersions).sort((a, b) => {
            const partsA = a.split('.').map(n => parseInt(n) || 0);
            const partsB = b.split('.').map(n => parseInt(n) || 0);
            for (let i = 0; i < Math.max(partsA.length, partsB.length); i++) {
                const diff = (partsB[i] || 0) - (partsA[i] || 0);
                if (diff !== 0) return diff;
            }
            return 0;
        });

        // Update Tab buttons
        document.querySelectorAll('.mod-details-tab').forEach(btn => {
            const isActive = btn.textContent.toLowerCase().includes(tab);
            btn.style.background = isActive ? 'var(--gold)' : 'transparent';
            btn.style.color = isActive ? 'var(--bg-dark)' : 'var(--text-secondary)';
        });

        // Update content
        const tabContent = document.getElementById('mod-details-tab-content');
        if (tabContent) {
            tabContent.innerHTML = '<div class="loading"><div class="spinner" style="margin: 20px auto;"></div></div>';
            tabContent.innerHTML = await renderModDetailsTabContent(mod, versions, allLoaders, sortedMcVersions);

            // Setze Dropdown-Werte explizit nach dem Rendern (für Versions-Tab)
            if (tab === 'versions') {
                setTimeout(() => {
                    const loaderSelect = document.getElementById('version-filter-loader');
                    const mcSelect = document.getElementById('version-filter-mc');

                    if (loaderSelect && modDetailsVersionFilter.loader) {
                        loaderSelect.value = modDetailsVersionFilter.loader;
                        debugLog(`Set loader dropdown to: ${modDetailsVersionFilter.loader}`, 'info');
                    }
                    if (mcSelect && modDetailsVersionFilter.mcVersion) {
                        mcSelect.value = modDetailsVersionFilter.mcVersion;
                        debugLog(`Set MC version dropdown to: ${modDetailsVersionFilter.mcVersion}`, 'info');
                    }
                }, 50);
            }
        }
    }
}

function filterModVersions() {
    const loaderSelect = document.getElementById('version-filter-loader');
    const mcSelect = document.getElementById('version-filter-mc');

    modDetailsVersionFilter.loader = loaderSelect ? loaderSelect.value : '';
    modDetailsVersionFilter.mcVersion = mcSelect ? mcSelect.value : '';

    // Re-render versions tab
    switchModDetailsTab('versions');
}

function toggleSnapshotFilter() {
    modDetailsVersionFilter.includeSnapshots = !modDetailsVersionFilter.includeSnapshots;
    // Re-render versions tab
    switchModDetailsTab('versions');
}

function openGalleryImage(url, index, previewUrl = '') {
    const normalizedUrl = normalizeMediaUrl(url);
    if (!normalizedUrl) return;
    const normalizedPreview = normalizeMediaUrl(previewUrl);

    closeGalleryImageModal();

    const modal = document.createElement('div');
    modal.id = 'gallery-image-modal';
    modal.style.cssText = `
        position: fixed;
        inset: 0;
        background: rgba(0, 0, 0, 0.92);
        display: flex;
        align-items: center;
        justify-content: center;
        z-index: 12000;
        padding: 20px;
    `;

    // Erst die (bereits geladene) Vorschau groß anzeigen, dann gegen das Originalbild tauschen
    const hasSeparatePreview = !!normalizedPreview && normalizedPreview !== normalizedUrl;
    const initialSrc = hasSeparatePreview ? normalizedPreview : normalizedUrl;
    modal.innerHTML = `
        <button onclick="closeGalleryImageModal()"
                style="position:absolute;top:14px;right:18px;z-index:1;background:rgba(0,0,0,0.4);border:1px solid rgba(255,255,255,0.4);color:white;border-radius:6px;padding:6px 10px;cursor:pointer;font-size:14px;">
            <i class="bi bi-x-lg"></i> Close
        </button>
        <div style="width:min(96vw, 1800px);height:90vh;display:flex;align-items:center;justify-content:center;">
            <img src="${escapeAttr(initialSrc)}"
                 alt="Gallery image ${index + 1}"
                 referrerpolicy="no-referrer"
                 style="width:100%;height:100%;object-fit:contain;border-radius:8px;"
                 data-fallback="fullscreen">
        </div>
    `;

    if (hasSeparatePreview) {
        const fullImage = new Image();
        fullImage.referrerPolicy = 'no-referrer';
        fullImage.onload = () => {
            const shown = modal.querySelector('img');
            if (shown && document.body.contains(modal)) shown.src = normalizedUrl;
        };
        fullImage.src = normalizedUrl;
    }

    // Das Bild füllt die ganze Fläche (object-fit: contain) → jeder Klick schließt die Ansicht
    modal.addEventListener('click', () => closeGalleryImageModal());

    galleryModalKeyHandler = (event) => {
        if (event.key === 'Escape') {
            closeGalleryImageModal();
        }
    };
    document.addEventListener('keydown', galleryModalKeyHandler);
    document.body.appendChild(modal);
}

function closeGalleryImageModal() {
    const modal = document.getElementById('gallery-image-modal');
    if (modal) modal.remove();
    if (galleryModalKeyHandler) {
        document.removeEventListener('keydown', galleryModalKeyHandler);
        galleryModalKeyHandler = null;
    }
}

// Markdown (inkl. eingebettetem HTML) → bereinigtes HTML, siehe markdown.js
function formatMarkdown(text) {
    if (!text) return '';
    try {
        return window.LionMarkdown.renderMarkdown(text);
    } catch (error) {
        debugLog('Markdown rendering failed: ' + error, 'error');
        return `<p style="white-space: pre-wrap;">${escapeHtml(text)}</p>`;
    }
}

// ── Installation aus der Detailansicht ───────────────────────────────────────
const DETAILS_KIND_LABELS = { mods: 'Mod', resourcepacks: 'Resource Pack', shaderpacks: 'Shader Pack', modpacks: 'Modpack' };

/** Inhaltsart des in der Detailansicht geöffneten Projekts (laut Modrinth-Projekttyp). */
function getDetailsContentKind(mod) {
    switch (String(mod?.project_type || '').toLowerCase()) {
        case 'mod': return 'mods';
        case 'resourcepack': return 'resourcepacks';
        case 'shader': return 'shaderpacks';
        case 'modpack': return 'modpacks';
        default:
            return ['resourcepacks', 'shaderpacks', 'modpacks'].includes(currentContentType) ? currentContentType : 'mods';
    }
}

/** Installieren ist möglich mit offenem Profil, im Server-Kontext oder per Profilauswahl. */
function canInstallFromDetails() {
    return !!(currentProfile || serverBrowserContext || profiles.length > 0);
}

/** Ist das Projekt im aktuellen Profil installiert? (gleiche Regeln wie im Content Browser) */
function isProjectInstalledInCurrentProfile(mod, kind) {
    const keys = [mod.id, mod.slug, mod.name].map(normalizePackKey).filter(Boolean);
    const set = kind === 'resourcepacks' ? installedResourcePackNames
        : kind === 'shaderpacks' ? installedShaderPackNames
            : installedModIds;
    return keys.some(key => set.has(key));
}

function setDetailsInstallButtonInstalled() {
    const button = document.getElementById('mod-details-install-btn');
    if (!button) return;
    button.innerHTML = '<i class="bi bi-check"></i> Installed';
    button.className = 'btn btn-secondary';
    button.disabled = true;
    button.style.opacity = '0.6';
    button.style.cursor = 'not-allowed';
}

async function updateDetailsInstallButtonState(mod) {
    if (!mod) return;
    const kind = getDetailsContentKind(mod);
    if (kind === 'modpacks') return; // Modpacks werden als eigenes Profil installiert
    let installed = false;
    try {
        if (serverBrowserContext) {
            await loadServerInstalledProjectIds();
            installed = serverInstalledProjectIds.has(mod.id);
        } else if (currentProfile) {
            if (kind === 'resourcepacks') await loadInstalledResourcePackNames();
            else if (kind === 'shaderpacks') await loadInstalledShaderPackNames();
            else await loadInstalledModIds();
            installed = isProjectInstalledInCurrentProfile(mod, kind);
        }
    } catch (error) {
        debugLog('Could not determine install state: ' + error, 'error');
    }
    // Nur anwenden, wenn noch dasselbe Projekt angezeigt wird
    if (installed && currentModDetails?.mod?.id === mod.id) setDetailsInstallButtonInstalled();
}

/** Markiert eine sichtbare Karte im Content Browser als installiert. */
function markBrowserCardInstalled(projectId) {
    document.querySelectorAll('.mod-card').forEach(card => {
        if (card.dataset.modId !== projectId) return;
        const btn = card.querySelector('.install-btn');
        if (btn) {
            btn.outerHTML = '<button class="btn btn-secondary" disabled style="opacity: 0.5; cursor: not-allowed;">Installiert</button>';
        }
        card.style.opacity = '0.7';
        card.style.borderColor = '#555';
        const nameRow = card.querySelector('.mod-name')?.parentElement;
        if (nameRow && !nameRow.querySelector('[data-installed-badge]')) {
            nameRow.insertAdjacentHTML('beforeend',
                '<span data-installed-badge style="background: #4caf50; color: white; font-size: 10px; padding: 2px 5px; border-radius: 3px;"><i class="bi bi-check"></i> Installiert</span>');
        }
    });
}

/**
 * Installiert ein Projekt aus der Detailansicht ins aktuelle Profil bzw. den aktuellen Server.
 * versionId = null → passende Version fürs Profil, sonst genau diese Version.
 * Rückmeldungen ausschließlich über Toasts. Gibt true bei Erfolg zurück.
 */
async function installProjectFromDetails(projectId, versionId = null, button = null) {
    const mod = currentModDetails?.mod;
    const source = currentModDetails?.source || 'modrinth';
    const kind = getDetailsContentKind(mod);
    const label = DETAILS_KIND_LABELS[kind] || 'Inhalt';
    const displayName = mod?.name || label;

    const setBusy = (busy) => {
        if (!button) return;
        if (busy) {
            button.dataset.originalHtml = button.innerHTML;
            button.innerHTML = '<span class="lion-spin"><i class="bi bi-arrow-repeat"></i></span> Installing...';
            button.disabled = true;
        } else {
            button.innerHTML = button.dataset.originalHtml || 'Install';
            button.disabled = false;
        }
    };

    // Server-Kontext: Inhalt landet im lokalen Server
    if (serverBrowserContext) {
        const ctx = serverBrowserContext;
        setBusy(true);
        try {
            const result = await invoke('install_server_content', { serverId: ctx.serverId, projectId, versionId });
            await loadServerInstalledProjectIds(true);
            browserResultsCache.clear();
            markBrowserCardInstalled(projectId);
            showToast(`${result.installed?.[0] || displayName} zu "${ctx.serverName}" hinzugefügt`, 'success', 3500);
            (result.warnings || []).forEach(w => showToast(w, 'warning', 5000));
            return true;
        } catch (error) {
            setBusy(false);
            showToast('Installation fehlgeschlagen: ' + error, 'error', 5000);
            return false;
        }
    }

    // Modpacks werden als eigenes Profil installiert
    if (kind === 'modpacks') {
        await installModpack(projectId);
        return false;
    }

    let profile = currentProfile;
    if (!profile) {
        if (!profiles.length) {
            showToast('Bitte erstelle zuerst ein Profil!', 'warning', 4000);
            return false;
        }
        profile = await showProfileSelectDialog();
        if (!profile) return false;
    }

    setBusy(true);
    try {
        if (kind === 'resourcepacks') {
            await invoke('install_resourcepack', { profileId: profile.id, packId: projectId, versionId });
        } else if (kind === 'shaderpacks') {
            await invoke('install_shaderpack', { profileId: profile.id, packId: projectId, versionId });
        } else {
            await invoke('install_mod', { profileId: profile.id, modId: projectId, versionId, source });
        }

        // "Installiert"-Status überall aktualisieren (Content Browser, Profil-Listen)
        invalidateProfileContentCaches(profile.id);
        browserResultsCache.clear();
        if (currentProfile && currentProfile.id === profile.id) {
            if (kind === 'resourcepacks') await loadInstalledResourcePackNames(true);
            else if (kind === 'shaderpacks') await loadInstalledShaderPackNames(true);
            else await loadInstalledModIds(true);
            markBrowserCardInstalled(projectId);
        }

        showToast(`${displayName} wurde zu "${profile.name}" hinzugefügt`, 'success', 3500);
        return true;
    } catch (error) {
        debugLog(`Install from details failed: ${error}`, 'error');
        setBusy(false);
        showToast(`${label}-Installation fehlgeschlagen: ${error}`, 'error', 5000);
        return false;
    }
}

/** Großer Install-Button oben rechts: passende Version fürs Profil installieren. */
async function installModFromDetails(projectId, button = null) {
    const ok = await installProjectFromDetails(projectId, null, button || document.getElementById('mod-details-install-btn'));
    if (!ok) return;
    setDetailsInstallButtonInstalled();
    if (currentModDetailsTab === 'versions') await switchModDetailsTab('versions');
}

/** Versionen-Tab: genau diese Version installieren. */
async function installModVersion(modId, versionId, button = null) {
    const ok = await installProjectFromDetails(modId, versionId, button);
    if (!ok) return;
    setDetailsInstallButtonInstalled();
    await switchModDetailsTab('versions'); // "INSTALLED"-Markierung aktualisieren
}

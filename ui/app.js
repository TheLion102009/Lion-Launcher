// Debug-Log-Funktion für visuelles Feedback
function debugLog(message, type = 'info') {
    const time = new Date().toLocaleTimeString();
    console.log(`[${time}] ${message}`);
}

// Rechtsklick-Kontextmenü deaktivieren (außer bei speziellen Elementen)
document.addEventListener('contextmenu', (e) => {
    // Erlaube Rechtsklick nur bei Elementen mit data-context-menu Attribut
    if (!e.target.closest('[data-context-menu]')) {
        e.preventDefault();
    }
});

// Tauri 2 API Import - mit Fallback
let invoke;
try {
    if (window.__TAURI_INTERNALS__) {
        invoke = window.__TAURI_INTERNALS__.invoke;
        debugLog('Tauri 2 API loaded (__TAURI_INTERNALS__)', 'success');
    } else if (window.__TAURI__ && window.__TAURI__.core) {
        invoke = window.__TAURI__.core.invoke;
        debugLog('Tauri 2 API loaded (__TAURI__.core)', 'success');
    } else if (window.__TAURI__ && window.__TAURI__.tauri) {
        invoke = window.__TAURI__.tauri.invoke;
        debugLog('Tauri 1 API loaded (fallback)', 'success');
    } else {
        throw new Error('Tauri API not found!');
    }
} catch (e) {
    console.error('Failed to load Tauri API:', e.message);
    invoke = async (cmd, _args) => {
        console.error('Mock invoke:', cmd);
        throw new Error('Tauri API not available');
    };
}

// State
let currentPage = 'profiles';
let currentProfile = null;
let profiles = [];
let currentUsername = 'Guest';
let selectedFilters = {
    version: '',
    loader: '',
    sort: 'relevance'
};
let currentModSearchQuery = '';
let currentModPage = 0;
const MODS_PER_PAGE = 20;

// Theme State
let currentTheme = 'dark';
let currentAccentColor = 'gold';

// Initialize
document.addEventListener('DOMContentLoaded', async () => {
    debugLog('Lion Launcher starting...', 'info');

    const grid = document.getElementById('profiles-grid');
    if (grid) {
        grid.innerHTML = `
            <div style="grid-column: 1 / -1; text-align: center; padding: 40px;">
                <div style="font-size: 36px; margin-bottom: 15px;">🦁</div>
                <p style="color: var(--gold);">Lion Launcher wird initialisiert...</p>
            </div>
        `;
    }

    try {
        debugLog('Initializing launcher directories...', 'info');
        await invoke('initialize_launcher').catch(err => {
            debugLog('Initialize warning (non-critical): ' + err, 'error');
        });

        debugLog('Loading settings...', 'info');
        loadSettings();

        debugLog('Loading accounts...', 'info');
        await loadAccounts();

        setupNavigation();
        setupModals();

        await loadProfiles();

        debugLog('Loading Minecraft versions...', 'info');
        await loadMinecraftVersions();

        setupSearch();

        debugLog('Lion Launcher ready!', 'success');
    } catch (error) {
        debugLog('Initialization error: ' + error, 'error');
        if (grid) {
            grid.innerHTML = `
                <div style="grid-column: 1 / -1; text-align: center; padding: 40px; color: var(--text-secondary);">
                    <div style="font-size: 36px; margin-bottom: 15px;">❌</div>
                    <p>Fehler beim Starten: ${error}</p>
                </div>
            `;
        }
    }
});

// ==================== TOAST NOTIFICATIONS ====================
function showToast(message, type = 'info', duration = 3000) {
    const toast = document.createElement('div');
    const icon = type === 'success' ? '✅' : type === 'error' ? '❌' : type === 'warning' ? '⚠️' : 'ℹ️';
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

    // Füge Animation CSS hinzu wenn noch nicht vorhanden
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

    // Nach duration verschwinden lassen
    setTimeout(() => {
        toast.style.animation = 'slideOut 0.3s ease-in';
        setTimeout(() => {
            if (toast.parentElement) {
                toast.remove();
            }
        }, 300);
    }, duration);
}

// Navigation
function setupNavigation() {
    document.querySelectorAll('.nav-item[data-page]').forEach(item => {
        item.addEventListener('click', () => {
            const page = item.dataset.page;
            switchPage(page);
        });
    });
}

function switchPage(page) {
    // Stoppe Mods-Watcher wenn Seite gewechselt wird
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

    // Zeige Back-Button im Mod-Browser wenn von einem Profil kommend
    const backBtn = document.getElementById('back-to-profile-btn');
    if (backBtn) {
        if (page === 'mods' && currentProfile) {
            backBtn.style.display = 'block';
        } else {
            backBtn.style.display = 'none';
        }
    }

    // Aktualisiere Cache wenn Mod-Browser geöffnet wird und rendere neu
    if (page === 'mods') {
        loadInstalledModIds().then(() => {
            // Lade Mods neu mit aktuellem Cache
            if (currentModSearchQuery) {
                searchMods(currentModSearchQuery, currentModPage);
            } else {
                loadPopularMods(currentModPage);
            }
        });
    }
}

function backToProfileFromModBrowser() {
    if (currentProfile) {
        showProfileDetails(currentProfile.id);
        switchPage('profiles');
    } else {
        switchPage('profiles');
    }
}

// Profiles
async function loadProfiles() {
    // Stoppe Mods-Watcher wenn Profil-Ansicht verlassen wird
    if (typeof stopModsWatcher === 'function') {
        stopModsWatcher();
    }
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
                    <div style="font-size: 36px; margin-bottom: 15px;">⚠️</div>
                    <p style="margin-bottom: 10px;">Fehler beim Laden der Profile</p>
                    <p style="font-size: 14px; color: #888;">${error}</p>
                    <button class="btn" onclick="loadProfiles()" style="margin-top: 20px;">
                        Erneut versuchen
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
        grid.innerHTML = `
            <div style="grid-column: 1 / -1; text-align: center; padding: 60px 20px;">
                <div style="font-size: 48px; margin-bottom: 20px;">📦</div>
                <h2 style="color: var(--gold); margin-bottom: 15px;">Noch keine Profile erstellt</h2>
                <p style="color: var(--text-secondary); margin-bottom: 30px;">
                    Erstelle dein erstes Profil, um Minecraft zu spielen!
                </p>
                <button class="btn" onclick="document.getElementById('create-profile-btn').click()" 
                        style="font-size: 18px; padding: 15px 40px;">
                    + Profil erstellen
                </button>
            </div>
        `;
        return;
    }

    grid.innerHTML = profiles.map(profile => {
        // Modloader-Name formatieren (erster Buchstabe groß)
        const loaderName = profile.loader.loader.charAt(0).toUpperCase() + profile.loader.loader.slice(1);
        const loaderDisplay = profile.loader.loader === 'vanilla' ? 'Vanilla' : loaderName;

        return `
        <div class="profile-card" data-context-menu="profile" data-profile-id="${profile.id}"
             onclick="showProfileDetails('${profile.id}')"
             oncontextmenu="showProfileContextMenu(event, '${profile.id}')">
            <div class="profile-icon" style="font-size: 48px;">
                ${profile.icon_path ? '🎮' : '📦'}
            </div>
            <div class="profile-name">${profile.name}</div>
            <div class="profile-info">${loaderDisplay} ${profile.minecraft_version}</div>
            <button class="btn" onclick="event.stopPropagation(); launchProfile('${profile.id}')" 
                    style="width: 100%; margin-top: 15px; font-size: 14px; padding: 12px;">▶ Play</button>
        </div>
    `}).join('');
}

// Profil-Kontextmenü bei Rechtsklick
function showProfileContextMenu(event, profileId) {
    event.preventDefault();
    event.stopPropagation();

    // Entferne altes Kontextmenü falls vorhanden
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
            ⚙️ Einstellungen
        </div>
        <div onclick="deleteProfile('${profileId}')" 
             style="padding: 10px 20px; cursor: pointer; color: #f44336; display: flex; align-items: center; gap: 10px;"
             onmouseover="this.style.background='var(--bg-light)'" 
             onmouseout="this.style.background='transparent'">
            🗑️ Löschen
        </div>
    `;

    document.body.appendChild(menu);

    // Schließe Menü bei Klick außerhalb
    const closeMenu = (e) => {
        if (!menu.contains(e.target)) {
            menu.remove();
            document.removeEventListener('click', closeMenu);
        }
    };
    setTimeout(() => document.addEventListener('click', closeMenu), 10);
}

// Öffne Profil-Einstellungen als eigenes Modal-Fenster
function openProfileSettings(profileId) {
    const menu = document.getElementById('profile-context-menu');
    if (menu) menu.remove();

    const profile = profiles.find(p => p.id === profileId);
    if (!profile) return;

    // Entferne altes Modal falls vorhanden
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
                <h2 style="color: var(--gold); margin: 0; font-size: 18px;">⚙️ Profil-Einstellungen</h2>
                <button onclick="closeProfileSettingsModal()" 
                        style="background: none; border: none; color: var(--text-secondary); font-size: 24px; cursor: pointer; padding: 0; line-height: 1;">
                    ✕
                </button>
            </div>
            
            <!-- Scrollbarer Inhalt -->
            <div style="padding: 20px; overflow-y: auto; flex: 1;">
                
                <!-- Profil-Bild -->
                <div style="display: flex; gap: 15px; align-items: center; background: var(--bg-light); padding: 15px; border-radius: 8px; margin-bottom: 20px;">
                    <div id="profile-icon-preview" style="width: 60px; height: 60px; background: var(--bg-medium); border-radius: 8px; display: flex; align-items: center; justify-content: center; font-size: 30px; overflow: hidden;">
                        ${profile.icon_path ? `<img src="${profile.icon_path}" style="width: 100%; height: 100%; object-fit: cover;" alt="Icon">` : '📦'}
                    </div>
                    <div style="flex: 1;">
                        <input type="file" id="profile-icon-input" accept="image/*" onchange="previewProfileIcon(event)" style="display: none;">
                        <button class="btn btn-secondary" onclick="document.getElementById('profile-icon-input').click()" style="padding: 6px 12px; font-size: 12px;">
                            📷 Bild
                        </button>
                        <button class="btn btn-secondary" onclick="clearProfileIcon()" style="padding: 6px 12px; font-size: 12px; margin-left: 5px;">
                            ✕
                        </button>
                    </div>
                </div>
                
                <!-- Profilname -->
                <div style="margin-bottom: 15px;">
                    <label style="display: block; margin-bottom: 5px; color: var(--text-secondary); font-size: 13px;">Profilname</label>
                    <input type="text" value="${profile.name}" id="edit-profile-name"
                           style="width: 100%; padding: 10px; background: var(--bg-light); border: none; border-radius: 6px; color: var(--text-primary); font-size: 14px;">
                </div>
                
                <!-- Minecraft Version -->
                <div style="margin-bottom: 15px;">
                    <label style="display: block; margin-bottom: 5px; color: var(--text-secondary); font-size: 13px;">Minecraft Version</label>
                    <div style="display: flex; gap: 10px; align-items: center;">
                        <select id="edit-profile-mc-version" 
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
                
                <!-- Speicher -->
                <div style="margin-bottom: 15px;">
                    <label style="display: block; margin-bottom: 5px; color: var(--text-secondary); font-size: 13px;">Speicher (MB)</label>
                    <input type="number" value="${profile.memory_mb || 4096}" id="edit-profile-memory"
                           style="width: 100%; padding: 10px; background: var(--bg-light); border: none; border-radius: 6px; color: var(--text-primary); font-size: 14px;">
                </div>
                
                <!-- Java Argumente -->
                <div style="margin-bottom: 15px;">
                    <label style="display: block; margin-bottom: 5px; color: var(--text-secondary); font-size: 13px;">Java Argumente</label>
                    <textarea id="edit-profile-java-args" rows="2"
                              style="width: 100%; padding: 10px; background: var(--bg-light); border: none; border-radius: 6px; color: var(--text-primary); font-family: monospace; font-size: 12px; resize: vertical;"
                              placeholder="-XX:+UseG1GC">${(profile.java_args || []).join(' ')}</textarea>
                </div>
                
                <!-- Spielverzeichnis -->
                <div style="margin-bottom: 15px;">
                    <label style="display: block; margin-bottom: 5px; color: var(--text-secondary); font-size: 13px;">Spielverzeichnis</label>
                    <div style="display: flex; gap: 8px;">
                        <input type="text" value="${profile.game_dir}" readonly
                               style="flex: 1; padding: 10px; background: var(--bg-light); border: none; border-radius: 6px; color: var(--text-secondary); font-size: 12px;">
                        <button class="btn btn-secondary" onclick="openProfileFolder('${profile.id}')" style="padding: 10px 15px;">
                            📁
                        </button>
                    </div>
                </div>
            </div>
            
            <!-- Footer -->
            <div style="padding: 15px 20px; border-top: 1px solid var(--bg-light); display: flex; justify-content: flex-end; gap: 10px;">
                <button class="btn btn-secondary" onclick="closeProfileSettingsModal()" style="padding: 10px 20px;">
                    Abbrechen
                </button>
                <button class="btn" onclick="saveProfileSettingsFromModal('${profile.id}')" style="padding: 10px 25px;">
                    💾 Speichern
                </button>
            </div>
        </div>
    `;

    document.body.appendChild(modal);

    // Schließen bei Klick auf Hintergrund
    modal.addEventListener('click', (e) => {
        if (e.target === modal) closeProfileSettingsModal();
    });

    // Escape-Taste zum Schließen
    const escHandler = (e) => {
        if (e.key === 'Escape') {
            closeProfileSettingsModal();
            document.removeEventListener('keydown', escHandler);
        }
    };
    document.addEventListener('keydown', escHandler);

    // Versionen laden
    setTimeout(() => populateEditVersionSelect(), 50);
}

function closeProfileSettingsModal() {
    const modal = document.getElementById('profile-settings-modal');
    if (modal) modal.remove();
    selectedProfileIcon = null;
}

async function saveProfileSettingsFromModal(profileId) {
    const nameInput = document.getElementById('edit-profile-name');
    const memoryInput = document.getElementById('edit-profile-memory');
    const mcVersionSelect = document.getElementById('edit-profile-mc-version');
    const loaderSelect = document.getElementById('edit-profile-loader');
    const loaderVersionSelect = document.getElementById('edit-profile-loader-version');
    const javaArgsTextarea = document.getElementById('edit-profile-java-args');

    if (!nameInput || !memoryInput) {
        showToast('Fehler: Formular nicht gefunden', 'error', 3000);
        return;
    }

    try {
        const updates = {
            name: nameInput.value,
            minecraft_version: mcVersionSelect?.value,
            loader: loaderSelect?.value,
            loader_version: loaderVersionSelect?.value,
            memory_mb: parseInt(memoryInput.value) || 4096,
            java_args: javaArgsTextarea?.value.split(' ').filter(a => a.trim()) || [],
            icon_path: selectedProfileIcon || null
        };

        await invoke('update_profile', {
            profileId: profileId,
            updates: updates
        });

        closeProfileSettingsModal();
        showToast('Profil-Einstellungen gespeichert!', 'success', 3000);
        selectedProfileIcon = null;

        // Reload profiles
        await loadProfiles();
    } catch (error) {
        debugLog('Failed to save settings: ' + error, 'error');
        showToast('Fehler beim Speichern: ' + error, 'error', 5000);
    }
}

async function launchProfile(profileId) {
    const profile = profiles.find(p => p.id === profileId);
    const profileName = profile ? profile.name : 'Unknown';

    if (!currentUsername || currentUsername === 'Guest') {
        alert('Bitte setze zuerst deinen Username in den Settings!');
        switchPage('settings');
        return;
    }

    debugLog('Launching: ' + profileName, 'info');

    // Zeige Fortschrittsanzeige
    const modalHTML = `
        <div id="launch-progress-modal" style="position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.9); display: flex; align-items: center; justify-content: center; z-index: 10000;">
            <div style="background: var(--bg-dark); border: 2px solid var(--gold); border-radius: 10px; padding: 40px; text-align: center; min-width: 400px;">
                <div style="font-size: 48px; margin-bottom: 20px;">🎮</div>
                <h2 style="color: var(--gold); margin: 0 0 20px 0;">Minecraft wird vorbereitet...</h2>
                <p style="color: var(--text-secondary); margin-bottom: 30px;" id="launch-status">
                    Lade Version-Info...
                </p>
                <div style="background: var(--bg-light); border-radius: 10px; height: 8px; overflow: hidden;">
                    <div id="launch-progress-bar" style="background: var(--gold); height: 100%; width: 0%; transition: width 0.3s;"></div>
                </div>
            </div>
        </div>
    `;

    const modalDiv = document.createElement('div');
    modalDiv.innerHTML = modalHTML;
    document.body.appendChild(modalDiv.firstElementChild);

    const updateProgress = (status, percent) => {
        const statusEl = document.getElementById('launch-status');
        const barEl = document.getElementById('launch-progress-bar');
        if (statusEl) statusEl.textContent = status;
        if (barEl) barEl.style.width = percent + '%';
    };

    try {
        updateProgress('Lade Version-Info...', 10);
        await new Promise(r => setTimeout(r, 300));

        updateProgress('Lade Minecraft herunter (Client, Libraries, Assets)...', 30);
        await new Promise(r => setTimeout(r, 300));

        updateProgress('Dies kann beim ersten Mal 1-2 Minuten dauern...', 50);

        await invoke('launch_profile', {
            profileId: profileId,
            username: currentUsername
        });

        updateProgress('Minecraft gestartet! ✓', 100);
        debugLog('Minecraft started successfully!', 'success');

        await new Promise(r => setTimeout(r, 1500));

        // Modal schließen
        const modal = document.getElementById('launch-progress-modal');
        if (modal) modal.remove();

    } catch (error) {
        debugLog('Launch failed: ' + error, 'error');

        // Fehler-Modal zeigen
        const modal = document.getElementById('launch-progress-modal');
        if (modal) {
            modal.innerHTML = `
                <div style="background: var(--bg-dark); border: 2px solid #f44336; border-radius: 10px; padding: 40px; text-align: center; max-width: 500px;">
                    <div style="font-size: 48px; margin-bottom: 20px;">❌</div>
                    <h2 style="color: #f44336; margin: 0 0 20px 0;">Launch fehlgeschlagen</h2>
                    <p style="color: var(--text-secondary); margin-bottom: 20px; word-break: break-word;">
                        ${error}
                    </p>
                    <button class="btn" onclick="document.getElementById('launch-progress-modal').remove()" style="padding: 12px 30px;">
                        OK
                    </button>
                </div>
            `;
        }
    }
}

function showMicrosoftLoginInfo() {
    const modalHTML = `
        <div style="position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.8); display: flex; align-items: center; justify-content: center; z-index: 10000;" onclick="this.remove()">
            <div style="background: var(--bg-dark); border: 2px solid var(--gold); border-radius: 10px; padding: 30px; max-width: 600px; max-height: 80vh; overflow-y: auto;" onclick="event.stopPropagation()">
                <h2 style="color: var(--gold); margin: 0 0 20px 0;">🔐 Microsoft-Login (Coming Soon)</h2>
                
                <p style="color: var(--text-secondary); margin-bottom: 20px;">
                    Der Lion Launcher wird einen <strong style="color: var(--gold);">echten Microsoft-Login</strong> implementieren!
                </p>
                
                <h3 style="color: var(--text-primary); margin-bottom: 10px;">Wie wird es funktionieren?</h3>
                
                <ol style="color: var(--text-secondary); margin: 0 0 20px 20px; line-height: 1.8;">
                    <li><strong>OAuth2-Flow</strong> - Browser öffnet sich mit Microsoft-Login</li>
                    <li><strong>Du meldest dich an</strong> - Mit deinem echten Microsoft-Account</li>
                    <li><strong>Token wird gespeichert</strong> - Verschlüsselt auf deinem PC</li>
                    <li><strong>Automatische Verlängerung</strong> - Token wird automatisch erneuert</li>
                    <li><strong>Offline-Support</strong> - Cached Token für Offline-Play</li>
                </ol>
                
                <h3 style="color: var(--text-primary); margin-bottom: 10px;">Was bekommst du?</h3>
                
                <ul style="color: var(--text-secondary); margin: 0 0 20px 20px; line-height: 1.8;">
                    <li>✅ <strong>Dein echter Minecraft-Account</strong></li>
                    <li>✅ <strong>Zugriff auf gekaufte Skins & Capes</strong></li>
                    <li>✅ <strong>Multiplayer auf allen Servern</strong></li>
                    <li>✅ <strong>Realms-Support</strong></li>
                    <li>✅ <strong>Account-Sicherheit</strong></li>
                </ul>
                
                <div style="background: var(--bg-light); border-left: 4px solid #4CAF50; padding: 15px; margin-top: 20px;">
                    <p style="color: var(--text-primary); margin: 0 0 5px 0;">
                        <strong>🔒 Sicherheit:</strong>
                    </p>
                    <p style="color: var(--text-secondary); margin: 0; font-size: 14px;">
                        Dein Passwort wird <strong>NIE</strong> im Launcher gespeichert!<br>
                        Nur Microsoft hat Zugriff auf deine Login-Daten.<br>
                        Der Launcher speichert nur einen verschlüsselten Access-Token.
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
    if (!confirm('Are you sure you want to delete this profile?')) {
        return;
    }

    try {
        const profileList = await invoke('delete_profile', { profileId });
        profiles = profileList.profiles || [];
        renderProfiles();
    } catch (error) {
        debugLog('Failed to delete profile: ' + error, 'error');
        alert('Failed to delete profile: ' + error);
    }
}

// Profile Details View
function showProfileDetails(profileId) {
    debugLog('Opening profile details: ' + profileId, 'info');

    const profile = profiles.find(p => p.id === profileId);
    if (!profile) {
        debugLog('Profile not found: ' + profileId, 'error');
        return;
    }

    currentProfile = profile;

    // Erstelle Details-View
    const grid = document.getElementById('profiles-grid');
    if (!grid) return;

    grid.innerHTML = `
        <div style="grid-column: 1 / -1;">
            <!-- Header mit zurück Button -->
            <div style="display: flex; align-items: center; margin-bottom: 30px; gap: 15px;">
                <button class="btn btn-secondary" onclick="loadProfiles()" style="padding: 10px 20px;">
                    ← Zurück
                </button>
                <div style="display: flex; align-items: center; gap: 15px; flex: 1;">
                    <div style="font-size: 64px;">
                        ${profile.icon_path ? '🎮' : '📦'}
                    </div>
                    <div>
                        <h2 style="color: var(--gold); margin: 0 0 5px 0;">${profile.name}</h2>
                        <p style="margin: 0; color: var(--text-secondary);">
                            Minecraft ${profile.minecraft_version} • ${profile.loader.loader} ${profile.loader.version}
                        </p>
                    </div>
                </div>
                <button class="btn" onclick="launchProfile('${profile.id}')" style="padding: 15px 40px; font-size: 18px;">
                    ▶ Play
                </button>
            </div>
            
            <!-- Tab Navigation -->
            <div style="display: flex; gap: 10px; margin-bottom: 20px; border-bottom: 2px solid var(--bg-light);">
                <button class="profile-tab active" data-tab="mods" onclick="switchProfileTab('mods')" 
                        style="padding: 10px 20px; background: none; border: none; color: var(--text-primary); cursor: pointer; border-bottom: 3px solid var(--gold);">
                    📦 Mods
                </button>
                <button class="profile-tab" data-tab="resourcepacks" onclick="switchProfileTab('resourcepacks')" 
                        style="padding: 10px 20px; background: none; border: none; color: var(--text-secondary); cursor: pointer; border-bottom: 3px solid transparent;">
                    🎨 Resource Packs
                </button>
                <button class="profile-tab" data-tab="shaderpacks" onclick="switchProfileTab('shaderpacks')" 
                        style="padding: 10px 20px; background: none; border: none; color: var(--text-secondary); cursor: pointer; border-bottom: 3px solid transparent;">
                    ✨ Shader Packs
                </button>
                <button class="profile-tab" data-tab="logs" onclick="switchProfileTab('logs')" 
                        style="padding: 10px 20px; background: none; border: none; color: var(--text-secondary); cursor: pointer; border-bottom: 3px solid transparent;">
                    📋 Logs
                </button>
            </div>
            
            <!-- Tab Content -->
            <div id="profile-tab-content" style="background: var(--bg-light); border-radius: 10px; padding: 20px; min-height: 400px;">
                ${renderProfileTabContent('mods', profile)}
            </div>
        </div>
    `;

    // Lade Mods automatisch nach dem Rendern
    setTimeout(() => {
        loadInstalledMods(profile.id);
        startModsWatcher(profile.id);
    }, 50);
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
        }
    }
}

function renderProfileTabContent(tabName, profile) {
    switch (tabName) {
        case 'mods':
            return `
                <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 15px;">
                    <h3 style="color: var(--gold); margin: 0;">Installierte Mods</h3>
                    <div style="display: flex; gap: 8px;">
                        <button class="btn btn-secondary" onclick="checkForModUpdates('${profile.id}')" style="padding: 8px 12px; font-size: 12px;">
                            🔍 Updates
                        </button>
                        <button class="btn btn-secondary" onclick="refreshInstalledMods('${profile.id}')" style="padding: 8px 12px; font-size: 12px;">
                            🔄
                        </button>
                        <button class="btn btn-secondary" onclick="openModsFolder('${profile.id}')" style="padding: 8px 12px; font-size: 12px;">
                            📁
                        </button>
                        <button class="btn" onclick="switchPage('mods')" style="padding: 8px 15px; font-size: 12px;">
                            + Mod
                        </button>
                    </div>
                </div>
                
                <!-- Bulk Actions Bar (hidden by default) -->
                <div id="bulk-actions-bar" style="display: none; background: var(--bg-light); border-radius: 8px; padding: 10px 15px; margin-bottom: 15px; align-items: center; gap: 15px;">
                    <label style="display: flex; align-items: center; gap: 8px; cursor: pointer; color: var(--text-secondary);">
                        <input type="checkbox" id="select-all-mods" onchange="toggleSelectAllMods()" style="width: 16px; height: 16px; cursor: pointer;">
                        <span id="selected-count">0 ausgewählt</span>
                    </label>
                    <div style="flex: 1;"></div>
                    <button class="btn btn-secondary" onclick="bulkActivateMods('${profile.id}')" style="padding: 6px 12px; font-size: 11px;">
                        ✅ Aktivieren
                    </button>
                    <button class="btn btn-secondary" onclick="bulkDeactivateMods('${profile.id}')" style="padding: 6px 12px; font-size: 11px;">
                        ⏸️ Deaktivieren
                    </button>
                    <button class="btn btn-secondary" onclick="bulkDeleteMods('${profile.id}')" style="padding: 6px 12px; font-size: 11px; color: #f44336;">
                        🗑️ Löschen
                    </button>
                </div>
                
                <div id="profile-mods-list" style="display: grid; gap: 8px;">
                    <div style="text-align: center; padding: 40px; color: var(--text-secondary);">
                        <div class="spinner" style="margin: 0 auto 15px;"></div>
                        <p>Lade installierte Mods...</p>
                    </div>
                </div>
            `;

        case 'resourcepacks':
            // Stoppe Mods-Watcher wenn Tab gewechselt wird
            stopModsWatcher();
            return `
                <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 20px;">
                    <h3 style="color: var(--gold); margin: 0;">Resource Packs</h3>
                    <button class="btn" onclick="openResourcePacksFolder('${profile.id}')" style="padding: 8px 20px;">
                        📁 Ordner öffnen
                    </button>
                </div>
                <div style="text-align: center; padding: 60px 20px; color: var(--text-secondary);">
                    <div style="font-size: 48px; margin-bottom: 15px;">🎨</div>
                    <p>Keine Resource Packs installiert</p>
                    <p style="font-size: 14px; margin-top: 10px;">
                        Lade Resource Packs herunter und ziehe sie in den Ordner
                    </p>
                </div>
            `;

        case 'shaderpacks':
            stopModsWatcher();
            return `
                <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 20px;">
                    <h3 style="color: var(--gold); margin: 0;">Shader Packs</h3>
                    <button class="btn" onclick="openShaderPacksFolder('${profile.id}')" style="padding: 8px 20px;">
                        📁 Ordner öffnen
                    </button>
                </div>
                <div style="text-align: center; padding: 60px 20px; color: var(--text-secondary);">
                    <div style="font-size: 48px; margin-bottom: 15px;">✨</div>
                    <p>Keine Shader Packs installiert</p>
                    <p style="font-size: 14px; margin-top: 10px;">
                        Benötigt Optifine oder Iris + Sodium<br>
                        Lade Shader Packs herunter und ziehe sie in den Ordner
                    </p>
                </div>
            `;

        case 'logs':
            stopModsWatcher();
            return `
                <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 20px;">
                    <h3 style="color: var(--gold); margin: 0;">Minecraft Logs</h3>
                    <div style="display: flex; gap: 10px;">
                        <button class="btn btn-secondary" onclick="loadLogs('${profile.id}')" style="padding: 8px 20px;">
                            🔄 Aktualisieren
                        </button>
                        <button class="btn btn-secondary" onclick="openLogsFolder('${profile.id}')" style="padding: 8px 20px;">
                            📁 Ordner öffnen
                        </button>
                    </div>
                </div>
                
                <!-- Log Type Selector -->
                <div style="display: flex; gap: 10px; margin-bottom: 15px;">
                    <button class="log-type-btn active" data-log="latest" onclick="switchLogType('latest', '${profile.id}')"
                            style="padding: 8px 15px; background: var(--gold); color: var(--bg-dark); border: none; border-radius: 5px; cursor: pointer;">
                        Latest Log
                    </button>
                    <button class="log-type-btn" data-log="debug" onclick="switchLogType('debug', '${profile.id}')"
                            style="padding: 8px 15px; background: var(--bg-dark); color: var(--text-secondary); border: 1px solid var(--bg-light); border-radius: 5px; cursor: pointer;">
                        Debug Log
                    </button>
                    <button class="log-type-btn" data-log="crash" onclick="switchLogType('crash', '${profile.id}')"
                            style="padding: 8px 15px; background: var(--bg-dark); color: var(--text-secondary); border: 1px solid var(--bg-light); border-radius: 5px; cursor: pointer;">
                        Crash Reports
                    </button>
                </div>
                
                <!-- Log Content -->
                <div id="log-content" style="background: #0d0d0d; border: 1px solid var(--bg-light); border-radius: 5px; padding: 15px; font-family: 'Courier New', monospace; font-size: 11px; height: 450px; overflow-y: auto; color: #0f0; white-space: pre-wrap; word-break: break-all;">
                    <div style="color: var(--text-secondary); text-align: center; padding: 40px;">
                        📋 Klicke auf "Aktualisieren" um Logs zu laden<br>
                        <span style="font-size: 10px; color: #666;">Logs erscheinen nach dem ersten Minecraft-Start</span>
                    </div>
                </div>
                
                <script>
                    // Auto-load logs when tab opens
                    setTimeout(() => loadLogs('${profile.id}'), 100);
                </script>
            `;

        default:
            return '<p style="text-align: center; color: var(--text-secondary); padding: 40px;">Inhalt nicht verfügbar</p>';
    }
}

// Helper functions for profile details

function openGameFolder(profileId) {
    debugLog('Opening game folder for: ' + profileId, 'info');
    alert('Ordner öffnen wird noch implementiert');
}

let currentLogType = 'latest';

async function loadLogs(profileId) {
    debugLog('Loading logs for profile: ' + profileId, 'info');

    const logContent = document.getElementById('log-content');
    if (!logContent) return;

    logContent.innerHTML = '<div style="color: var(--gold);">⏳ Lade Logs...</div>';

    try {
        const logs = await invoke('get_profile_logs', {
            profileId: profileId,
            logType: currentLogType
        });

        if (logs && logs.length > 0) {
            // Logs mit Syntax-Highlighting
            const formattedLogs = logs.split('\n').map(line => {
                if (line.includes('[ERROR]') || line.includes('ERROR') || line.includes('Exception')) {
                    return `<span style="color: #f44336;">${escapeHtml(line)}</span>`;
                } else if (line.includes('[WARN]') || line.includes('WARN')) {
                    return `<span style="color: #ff9800;">${escapeHtml(line)}</span>`;
                } else if (line.includes('[INFO]') || line.includes('INFO')) {
                    return `<span style="color: #4caf50;">${escapeHtml(line)}</span>`;
                } else if (line.includes('[DEBUG]') || line.includes('DEBUG')) {
                    return `<span style="color: #9e9e9e;">${escapeHtml(line)}</span>`;
                } else {
                    return `<span style="color: #ccc;">${escapeHtml(line)}</span>`;
                }
            }).join('\n');

            logContent.innerHTML = formattedLogs;
            logContent.scrollTop = logContent.scrollHeight;
            debugLog('Logs loaded: ' + logs.split('\n').length + ' lines', 'success');
        } else {
            logContent.innerHTML = `
                <div style="color: var(--text-secondary); text-align: center; padding: 40px;">
                    📋 Keine ${currentLogType} Logs gefunden<br>
                    <span style="font-size: 10px; color: #666;">Starte Minecraft um Logs zu generieren</span>
                </div>
            `;
        }
    } catch (error) {
        debugLog('Failed to load logs: ' + error, 'error');
        logContent.innerHTML = `
            <div style="color: #f44336; text-align: center; padding: 40px;">
                ❌ Fehler beim Laden der Logs<br>
                <span style="font-size: 10px; color: #666;">${error}</span>
            </div>
        `;
    }
}

function switchLogType(logType, profileId) {
    debugLog('Switching to log type: ' + logType, 'info');
    currentLogType = logType;

    // Update button styles
    document.querySelectorAll('.log-type-btn').forEach(btn => {
        if (btn.dataset.log === logType) {
            btn.style.background = 'var(--gold)';
            btn.style.color = 'var(--bg-dark)';
            btn.classList.add('active');
        } else {
            btn.style.background = 'var(--bg-dark)';
            btn.style.color = 'var(--text-secondary)';
            btn.classList.remove('active');
        }
    });

    // Load logs
    loadLogs(profileId);
}

async function openLogsFolder(profileId) {
    debugLog('Opening logs folder for: ' + profileId, 'info');
    try {
        await invoke('open_profile_folder', { profileId: profileId, subfolder: 'logs' });
    } catch (error) {
        debugLog('Failed to open folder: ' + error, 'error');
        alert('Konnte Ordner nicht öffnen: ' + error);
    }
}

function escapeHtml(text) {
    const div = document.createElement('div');
    div.textContent = text;
    return div.innerHTML;
}

function refreshLogs() {
    if (currentProfile) {
        loadLogs(currentProfile.id);
    }
}

function clearLogs() {
    if (confirm('Möchtest du wirklich alle Logs löschen?')) {
        debugLog('Clearing logs...', 'info');
        // TODO: Implement log clearing
    }
}

// ==================== MOD-VERWALTUNG ====================

let selectedMods = new Set();
let modsWatcherInterval = null;
let lastModsHash = '';

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
                loadInstalledMods(profileId);
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

// Generiert einen Hash aus der Mod-Liste um Änderungen zu erkennen
function generateModsHash(mods) {
    if (!mods || mods.length === 0) return 'empty';
    return mods.map(m => `${m.filename}:${m.disabled}`).sort().join('|');
}

async function loadInstalledMods(profileId) {
    debugLog('Loading installed mods for profile: ' + profileId, 'info');

    const modsList = document.getElementById('profile-mods-list');
    if (!modsList) return;

    selectedMods.clear();
    updateBulkActionsBar();

    try {
        const mods = await invoke('get_installed_mods', { profileId });

        if (!mods || mods.length === 0) {
            modsList.innerHTML = `
                <div style="text-align: center; padding: 60px 20px; color: var(--text-secondary);">
                    <div style="font-size: 48px; margin-bottom: 15px;">📦</div>
                    <p>Noch keine Mods installiert</p>
                    <p style="font-size: 14px; margin-top: 10px;">
                        Gehe zum <a href="#" onclick="switchPage('mods'); return false;" style="color: var(--gold);">Mod Browser</a> um Mods zu installieren
                    </p>
                </div>
            `;
            document.getElementById('bulk-actions-bar').style.display = 'none';
            return;
        }

        debugLog('Found ' + mods.length + ' installed mods', 'success');

        // Zeige Bulk Actions Bar
        document.getElementById('bulk-actions-bar').style.display = 'flex';

        modsList.innerHTML = mods.map(mod => {
            const iconUrl = mod.icon_url || null;
            const hasUpdate = mod.has_update;

            return `
            <div class="installed-mod-card" data-filename="${mod.filename}" style="background: var(--bg-dark); border: 1px solid ${mod.disabled ? '#666' : 'var(--bg-light)'}; border-radius: 8px; padding: 12px; display: flex; align-items: center; gap: 12px; ${mod.disabled ? 'opacity: 0.6;' : ''} transition: all 0.2s;">
                <!-- Checkbox -->
                <input type="checkbox" class="mod-checkbox" data-filename="${mod.filename}" 
                       onchange="toggleModSelection('${mod.filename}')"
                       style="width: 18px; height: 18px; cursor: pointer; flex-shrink: 0; accent-color: var(--gold);">
                
                <!-- Icon -->
                <div style="width: 44px; height: 44px; background: var(--bg-light); border-radius: 6px; display: flex; align-items: center; justify-content: center; flex-shrink: 0; overflow: hidden;">
                    ${iconUrl 
                        ? `<img src="${iconUrl}" style="width: 100%; height: 100%; object-fit: cover;" onerror="this.parentElement.innerHTML='<span style=\\'font-size: 22px;\\'>📦</span>'">` 
                        : `<span style="font-size: 22px;">📦</span>`
                    }
                </div>
                
                <!-- Info -->
                <div style="flex: 1; min-width: 0;">
                    <div style="display: flex; align-items: center; gap: 8px; flex-wrap: wrap;">
                        <h4 style="margin: 0; color: var(--text-primary); font-size: 14px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 200px;">
                            ${mod.name || mod.filename}
                        </h4>
                        ${mod.disabled ? '<span style="background: #f44336; color: white; font-size: 9px; padding: 2px 5px; border-radius: 3px;">DEAKTIVIERT</span>' : ''}
                        ${hasUpdate ? '<span style="background: var(--gold); color: var(--bg-dark); font-size: 9px; padding: 2px 5px; border-radius: 3px;">UPDATE</span>' : ''}
                    </div>
                    <p style="margin: 3px 0 0 0; color: var(--text-secondary); font-size: 11px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis;">
                        ${mod.filename}
                    </p>
                    ${mod.version ? `<p style="margin: 2px 0 0 0; color: var(--gold); font-size: 10px;">v${mod.version}</p>` : ''}
                </div>
                
                <!-- Actions -->
                <div style="display: flex; gap: 6px; flex-shrink: 0;">
                    <button class="btn btn-secondary" onclick="toggleMod('${profileId}', '${mod.filename}', ${mod.disabled})" 
                            style="padding: 5px 10px; font-size: 11px;" title="${mod.disabled ? 'Aktivieren' : 'Deaktivieren'}">
                        ${mod.disabled ? '✅' : '⏸️'}
                    </button>
                    <button class="btn btn-secondary" onclick="deleteMod('${profileId}', '${mod.filename}')" 
                            style="padding: 5px 10px; font-size: 11px; color: #f44336;" title="Löschen">
                        🗑️
                    </button>
                </div>
            </div>
        `}).join('');

        // Versuche Icons von Modrinth zu laden (asynchron)
        loadModIcons(mods);

    } catch (error) {
        debugLog('Failed to load installed mods: ' + error, 'error');
        modsList.innerHTML = `
            <div style="text-align: center; padding: 40px; color: #f44336;">
                <div style="font-size: 48px; margin-bottom: 15px;">❌</div>
                <p>Fehler beim Laden der Mods</p>
                <p style="font-size: 12px; color: var(--text-secondary);">${error}</p>
                <button class="btn btn-secondary" onclick="loadInstalledMods('${profileId}')" style="margin-top: 15px;">
                    🔄 Erneut versuchen
                </button>
            </div>
        `;
    }
}

function refreshInstalledMods(profileId) {
    loadInstalledMods(profileId);
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
        loadInstalledMods(profileId);
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
        loadInstalledMods(profileId);
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

    if (!confirm(`Möchtest du wirklich ${selectedMods.size} Mods löschen?`)) {
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
        await loadInstalledModIds(); // Cache aktualisieren
        selectedMods.clear();
        loadInstalledMods(profileId);
        showToast(`${count} Mods gelöscht!`, 'success', 3000);
    } catch (error) {
        debugLog('Failed to delete mods: ' + error, 'error');
        showToast('Fehler beim Löschen: ' + error, 'error', 3000);
    }
}

// Icon-Loading von Modrinth
async function loadModIcons(mods) {
    for (const mod of mods) {
        if (mod.mod_id) {
            try {
                const response = await fetch(`https://api.modrinth.com/v2/search?query=${encodeURIComponent(mod.mod_id)}&limit=1`);
                if (response.ok) {
                    const data = await response.json();
                    if (data.hits && data.hits.length > 0 && data.hits[0].icon_url) {
                        const card = document.querySelector(`.installed-mod-card[data-filename="${mod.filename}"]`);
                        if (card) {
                            const iconContainer = card.querySelector('div[style*="44px"]');
                            if (iconContainer) {
                                iconContainer.innerHTML = `<img src="${data.hits[0].icon_url}" style="width: 100%; height: 100%; object-fit: cover; border-radius: 4px;" onerror="this.parentElement.innerHTML='<span style=\\'font-size: 22px;\\'>📦</span>'">`;
                            }
                        }
                    }
                }
            } catch (e) {
                // Ignoriere Icon-Ladefehler
            }
        }
    }
}

// Update-Check
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
        const updates = await invoke('check_mod_updates', {
            profileId,
            mcVersion: profile.minecraft_version,
            loader: profile.loader.loader
        });

        document.getElementById('update-check-modal').remove();

        if (!updates || updates.length === 0) {
            alert('✅ Alle Mods sind aktuell!');
            return;
        }

        // Zeige Updates
        const updateModalHTML = `
            <div id="updates-modal" style="position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.8); display: flex; align-items: center; justify-content: center; z-index: 10000;" onclick="if(event.target === this) this.remove()">
                <div style="background: var(--bg-dark); border: 2px solid var(--gold); border-radius: 10px; padding: 25px; max-width: 500px; max-height: 80vh; overflow-y: auto;" onclick="event.stopPropagation()">
                    <h3 style="color: var(--gold); margin: 0 0 20px 0;">🔄 ${updates.length} Update(s) verfügbar</h3>
                    <div style="display: grid; gap: 10px;">
                        ${updates.map(u => `
                            <div style="background: var(--bg-light); border-radius: 8px; padding: 12px; display: flex; align-items: center; gap: 12px;">
                                <div style="width: 40px; height: 40px; background: var(--bg-dark); border-radius: 6px; display: flex; align-items: center; justify-content: center; overflow: hidden;">
                                    ${u.icon_url ? `<img src="${u.icon_url}" style="width: 100%; height: 100%; object-fit: cover;">` : '<span style="font-size: 18px;">📦</span>'}
                                </div>
                                <div style="flex: 1;">
                                    <p style="margin: 0; color: var(--text-primary); font-size: 13px;">${u.filename}</p>
                                    <p style="margin: 3px 0 0 0; font-size: 11px;">
                                        <span style="color: var(--text-secondary);">${u.current_version || '?'}</span>
                                        <span style="color: var(--gold);"> → ${u.latest_version || '?'}</span>
                                    </p>
                                </div>
                            </div>
                        `).join('')}
                    </div>
                    <p style="color: var(--text-secondary); font-size: 11px; margin: 15px 0 0 0; text-align: center;">
                        Lösche die alten Mods und installiere die neuen über den Mod Browser
                    </p>
                    <button class="btn" onclick="document.getElementById('updates-modal').remove()" style="width: 100%; margin-top: 15px; padding: 10px;">
                        Verstanden
                    </button>
                </div>
            </div>
        `;
        document.body.insertAdjacentHTML('beforeend', updateModalHTML);

    } catch (error) {
        document.getElementById('update-check-modal')?.remove();
        debugLog('Failed to check updates: ' + error, 'error');
        alert('Fehler beim Prüfen auf Updates: ' + error);
    }
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
        loadInstalledMods(profileId);

        // Toast-Benachrichtigung
        showToast(`Mod ${isCurrentlyDisabled ? 'aktiviert' : 'deaktiviert'}!`, 'success', 3000);

    } catch (error) {
        debugLog('Failed to toggle mod: ' + error, 'error');
        showToast('Fehler beim Umschalten der Mod: ' + error, 'error', 3000);
    }
}

async function deleteMod(profileId, filename) {
    if (!confirm(`Möchtest du "${filename}" wirklich löschen?`)) {
        return;
    }

    debugLog('Deleting mod: ' + filename, 'info');

    try {
        await invoke('delete_mod', { profileId, filename });

        debugLog('Mod deleted successfully', 'success');
        await loadInstalledModIds(); // Cache aktualisieren
        loadInstalledMods(profileId);

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
        alert('Konnte Mods-Ordner nicht öffnen: ' + error);
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
            preview.innerHTML = `<img src="${e.target.result}" style="width: 100%; height: 100%; object-fit: cover;">`;
        }
    };
    reader.readAsDataURL(file);
}

function clearProfileIcon() {
    selectedProfileIcon = null;
    const preview = document.getElementById('profile-icon-preview');
    if (preview) {
        preview.innerHTML = '📦';
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

    const currentVersion = currentProfile?.minecraft_version || '';
    const showSnapshotsEdit = document.getElementById('edit-show-snapshots')?.checked || false;

    const filtered = allMinecraftVersions.filter(v => {
        if (showSnapshotsEdit) return true;
        return v.version_type === 'release';
    });

    select.innerHTML = filtered.map(v =>
        `<option value="${v.id}" ${v.id === currentVersion ? 'selected' : ''}>${v.id}</option>`
    ).join('');
}

function updateEditVersionList() {
    populateEditVersionSelect();
}

async function updateEditLoaderVersions() {
    const loaderSelect = document.getElementById('edit-profile-loader');
    const versionSelect = document.getElementById('edit-profile-loader-version');
    const mcVersionSelect = document.getElementById('edit-profile-mc-version');

    if (!loaderSelect || !versionSelect) return;

    const loader = loaderSelect.value;
    const mcVersion = mcVersionSelect?.value || currentProfile?.minecraft_version;

    if (loader === 'vanilla') {
        versionSelect.innerHTML = '<option value="">-</option>';
        versionSelect.disabled = true;
        return;
    }

    versionSelect.disabled = false;
    versionSelect.innerHTML = '<option value="">Lade...</option>';

    try {
        if (loader === 'fabric') {
            const versions = await invoke('get_fabric_versions', { mcVersion });
            versionSelect.innerHTML = versions.map(v =>
                `<option value="${v.version}">${v.version}</option>`
            ).join('');
        } else {
            // Für Forge/NeoForge/Quilt
            versionSelect.innerHTML = '<option value="">Neueste</option>';
        }
    } catch (error) {
        console.error('Failed to load loader versions:', error);
        versionSelect.innerHTML = '<option value="">Neueste</option>';
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
function setupModals() {
    const createBtn = document.getElementById('create-profile-btn');
    const cancelBtn = document.getElementById('cancel-profile-btn');
    const saveBtn = document.getElementById('save-profile-btn');
    const loaderSelect = document.getElementById('profile-loader');

    if (createBtn) {
        createBtn.addEventListener('click', () => {
            debugLog('Create profile button clicked', 'info');
            const modal = document.getElementById('create-profile-modal');
            if (modal) {
                modal.classList.add('active');
                // Versionen nochmal aktualisieren wenn Modal geöffnet wird
                updateVersionSelects();
            }
        });
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

async function loadMinecraftVersions() {
    try {
        debugLog('Loading Minecraft versions...', 'info');
        allMinecraftVersions = await invoke('get_minecraft_versions');
        debugLog('Loaded ' + allMinecraftVersions.length + ' versions', 'success');

        updateVersionSelects();

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

function updateVersionSelects() {
    debugLog('Updating version selects...', 'info');

    if (allMinecraftVersions.length === 0) {
        debugLog('No versions loaded yet!', 'error');
        return;
    }

    // Debug: Check first version format
    if (allMinecraftVersions.length > 0) {
        const firstVersion = allMinecraftVersions[0];
        debugLog('First version: ' + JSON.stringify(firstVersion), 'info');
    }

    // Filter versions based on snapshot toggle
    // Support multiple version_type formats: "Release", "release", or type field
    const filteredVersions = showSnapshots
        ? allMinecraftVersions
        : allMinecraftVersions.filter(v => {
            const vType = v.version_type || v.type || '';
            return vType.toLowerCase() === 'release';
        });

    debugLog('Filtered to ' + filteredVersions.length + ' versions (snapshots: ' + showSnapshots + ')', 'info');

    // Update Profile Modal select (limit to 50 for performance)
    const profileSelect = document.getElementById('profile-mc-version');
    if (profileSelect) {
        debugLog('Found profile-mc-version select, updating...', 'success');
        profileSelect.innerHTML = filteredVersions.slice(0, 50).map(v =>
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
        alert('Please fill in all required fields');
        return;
    }

    try {
        debugLog('Creating profile: ' + name, 'info');
        const profileList = await invoke('create_profile', {
            name,
            minecraftVersion: mcVersion,
            loader,
            loaderVersion
        });

        profiles = profileList.profiles || [];
        renderProfiles();

        const modal = document.getElementById('create-profile-modal');
        if (modal) modal.classList.remove('active');
        nameInput.value = '';

        debugLog('Profile created successfully', 'success');
    } catch (error) {
        debugLog('Failed to create profile: ' + error, 'error');
        alert('Failed to create profile: ' + error);
    }
}

// Mod Browser
let installedModIds = new Set(); // Cache für installierte Mod-IDs

function setupSearch() {
    const searchInput = document.getElementById('mod-search');
    if (!searchInput) return;

    let searchTimeout;
    searchInput.addEventListener('input', (e) => {
        clearTimeout(searchTimeout);
        searchTimeout = setTimeout(() => {
            searchMods(e.target.value);
        }, 500);
    });

    document.querySelectorAll('[data-loader]').forEach(btn => {
        btn.addEventListener('click', () => {
            document.querySelectorAll('[data-loader]').forEach(b => b.classList.remove('active'));
            btn.classList.add('active');
            selectedFilters.loader = btn.dataset.loader;
            searchMods(searchInput.value);
        });
    });

    document.querySelectorAll('[data-sort]').forEach(btn => {
        btn.addEventListener('click', () => {
            document.querySelectorAll('[data-sort]').forEach(b => b.classList.remove('active'));
            btn.classList.add('active');
            selectedFilters.sort = btn.dataset.sort;
            searchMods(searchInput.value);
        });
    });

    const versionFilter = document.getElementById('filter-version');
    if (versionFilter) {
        versionFilter.addEventListener('change', (e) => {
            selectedFilters.version = e.target.value;
            searchMods(searchInput.value);
        });
    }

    // Lade automatisch beliebte Mods beim Start
    loadPopularMods();
}

// Lädt beliebte Mods (ohne Suchbegriff, sortiert nach Downloads)
async function loadPopularMods(page = 0) {
    const modList = document.getElementById('mod-list');
    if (!modList) return;

    currentModPage = page;
    currentModSearchQuery = '';

    modList.innerHTML = '<div class="loading"><div class="spinner" style="margin: 20px auto;"></div><p>Lade beliebte Mods...</p></div>';

    // Zuerst installierte Mods laden um sie zu markieren
    await loadInstalledModIds();

    try {
        // Suche nach beliebten Mods (leerer Query = alle, sortiert nach Downloads)
        const mods = await invoke('search_mods', {
            query: '',  // Leer für alle
            gameVersion: selectedFilters.version || null,
            loader: selectedFilters.loader || null,
            sortBy: 'downloads',  // Nach Downloads sortieren
            offset: page * MODS_PER_PAGE,
            limit: MODS_PER_PAGE
        });

        renderMods(mods, page);
    } catch (error) {
        debugLog('Failed to load popular mods: ' + error, 'error');
        modList.innerHTML = `
            <div class="loading" style="text-align: center; padding: 40px;">
                <div style="font-size: 48px; margin-bottom: 15px;">🔥</div>
                <p style="color: var(--gold); margin-bottom: 10px;">Beliebte Mods</p>
                <p style="color: var(--text-secondary);">Gib einen Suchbegriff ein oder versuche es später erneut</p>
            </div>
        `;
    }
}

// Lädt die IDs der installierten Mods für das aktive Profil
async function loadInstalledModIds() {
    installedModIds.clear();

    if (profiles.length === 0) return;

    const profile = profiles[0]; // Erstes Profil

    try {
        const mods = await invoke('get_installed_mods', { profileId: profile.id });
        debugLog('Loading installed mod IDs from ' + mods.length + ' mods', 'info');

        mods.forEach(mod => {
            // Die mod_id aus der Metadaten-Datei (Modrinth ID wie "AANobbMI")
            if (mod.mod_id) {
                installedModIds.add(mod.mod_id.toLowerCase());
                debugLog('  Added mod_id: ' + mod.mod_id.toLowerCase(), 'info');
            }

            // Der Mod-Name (z.B. "Sodium")
            if (mod.name) {
                const cleanName = mod.name.toLowerCase().replace(/\s+/g, '-');
                installedModIds.add(cleanName);
                // Auch nur den ersten Teil (vor dem ersten Leerzeichen)
                const firstName = mod.name.toLowerCase().split(' ')[0];
                if (firstName.length > 2) {
                    installedModIds.add(firstName);
                }
            }

            // Auch den Dateinamen parsen (z.B. "sodium-fabric-0.5.8" -> "sodium")
            if (mod.filename) {
                const cleanFilename = mod.filename
                    .toLowerCase()
                    .replace('.jar', '')
                    .replace('.disabled', '');
                // Ersten Teil vor dem ersten Bindestrich (oft der Mod-Slug)
                const firstPart = cleanFilename.split('-')[0];
                if (firstPart.length > 2) {
                    installedModIds.add(firstPart);
                }
            }
        });

        debugLog('Total installed mod IDs cached: ' + installedModIds.size + ' - ' + Array.from(installedModIds).join(', '), 'info');
    } catch (e) {
        debugLog('Could not load installed mods: ' + e, 'error');
    }
}

async function searchMods(query, page = 0) {
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

    try {
        const mods = await invoke('search_mods', {
            query,
            gameVersion: selectedFilters.version || null,
            loader: selectedFilters.loader || null,
            sortBy: selectedFilters.sort || 'relevance',
            offset: page * MODS_PER_PAGE,
            limit: MODS_PER_PAGE
        });

        renderMods(mods, page);
    } catch (error) {
        debugLog('Search failed: ' + error, 'error');
        modList.innerHTML = '<div class="loading">Suche fehlgeschlagen: ' + error + '</div>';
    }
}

function renderMods(mods, page = 0) {
    const list = document.getElementById('mod-list');
    if (!list) return;

    if (mods.length === 0 && page === 0) {
        list.innerHTML = '<div class="loading">Keine Mods gefunden</div>';
        return;
    }

    const modsHTML = mods.map(mod => {
        // Prüfe ob Mod bereits installiert ist
        const modSlug = mod.slug ? mod.slug.toLowerCase() : '';
        const modName = mod.name ? mod.name.toLowerCase().replace(/\s+/g, '-') : '';
        const modId = mod.id ? mod.id.toLowerCase() : '';
        const modFirstName = mod.name ? mod.name.toLowerCase().split(' ')[0] : '';

        // Exakte Matches prüfen
        const isInstalled = installedModIds.has(modSlug) ||
                           installedModIds.has(modName) ||
                           installedModIds.has(modId) ||
                           installedModIds.has(modFirstName) ||
                           // Auch prüfen ob Slug im Cache enthalten ist
                           (modSlug && Array.from(installedModIds).some(id => id === modSlug || modSlug === id));

        return `
            <div class="mod-card" style="${isInstalled ? 'opacity: 0.7; border-color: #555;' : ''}">
                <div class="mod-icon">
                    ${mod.icon_url ? `<img src="${mod.icon_url}" alt="${mod.name}">` : '📦'}
                </div>
                <div class="mod-info">
                    <div class="mod-name" style="display: flex; align-items: center; gap: 10px;">
                        ${mod.name}
                        ${isInstalled ? '<span style="background: #4caf50; color: white; font-size: 10px; padding: 2px 8px; border-radius: 3px;">✓ Installiert</span>' : ''}
                    </div>
                    <div class="mod-author">by ${mod.author}</div>
                    <div class="mod-description">${mod.description}</div>
                    <div class="mod-stats">
                        <span>📥 ${formatNumber(mod.downloads)} downloads</span>
                        <span>🏷️ ${mod.categories.slice(0, 3).join(', ')}</span>
                    </div>
                </div>
                ${isInstalled 
                    ? `<button class="btn btn-secondary" disabled style="align-self: center; opacity: 0.5; cursor: not-allowed;">Installiert</button>`
                    : `<button class="btn" onclick="installMod('${mod.id}', '${mod.source}')" style="align-self: center;">Install</button>`
                }
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
                    ${mods.length < MODS_PER_PAGE ? 'disabled style="opacity: 0.5; cursor: not-allowed;"' : ''}>
                Nächste →
            </button>
        </div>
    `;

    list.innerHTML = modsHTML + paginationHTML;
}

function previousModPage() {
    if (currentModPage > 0) {
        if (currentModSearchQuery) {
            searchMods(currentModSearchQuery, currentModPage - 1);
        } else {
            loadPopularMods(currentModPage - 1);
        }
    }
}

function nextModPage() {
    if (currentModSearchQuery) {
        searchMods(currentModSearchQuery, currentModPage + 1);
    } else {
        loadPopularMods(currentModPage + 1);
    }
}

async function installMod(modId, source) {
    if (profiles.length === 0) {
        alert('Bitte erstelle zuerst ein Profil!');
        switchPage('profiles');
        return;
    }

    // Zeige Profil-Auswahl wenn mehrere Profile vorhanden
    const profile = profiles[0]; // TODO: Profil-Auswahl implementieren

    debugLog('Installing mod ' + modId + ' to profile ' + profile.name + ' (' + profile.minecraft_version + ' ' + profile.loader.loader + ')', 'info');

    try {
        // Installiere Mod - Backend findet automatisch die passende Version für das Profil
        await invoke('install_mod', {
            profileId: profile.id,
            modId: modId,
            versionId: null,  // Backend findet passende Version für MC-Version + Loader
            source: source
        });

        debugLog('Mod installed successfully!', 'success');

        // Aktualisiere Cache für installierte Mods
        await loadInstalledModIds();

        // Aktualisiere Mod-Liste um "Installiert" Badge zu zeigen
        if (currentModSearchQuery) {
            searchMods(currentModSearchQuery, currentModPage);
        } else {
            loadPopularMods(currentModPage);
        }

        // Toast-Benachrichtigung statt Modal
        showToast(`Mod erfolgreich zu "${profile.name}" hinzugefügt!`, 'success', 3000);

    } catch (error) {
        debugLog('Install failed: ' + error, 'error');

        // Toast-Benachrichtigung für Fehler
        showToast('Mod-Installation fehlgeschlagen: ' + error, 'error', 5000);
    }
}

function showModInstallError(title, htmlContent) {
    const modalHTML = `
        <div style="position: fixed; top: 0; left: 0; right: 0; bottom: 0; background: rgba(0,0,0,0.8); display: flex; align-items: center; justify-content: center; z-index: 10000;" onclick="this.remove()">
            <div style="background: var(--bg-dark); border: 2px solid #f44336; border-radius: 10px; padding: 30px; max-width: 500px;" onclick="event.stopPropagation()">
                <h2 style="color: #f44336; margin: 0 0 20px 0;">❌ ${title}</h2>
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

// Settings
function loadSettings() {
    const username = localStorage.getItem('username') || 'Guest';
    currentUsername = username;

    const usernameDisplay = document.getElementById('username-display');
    if (usernameDisplay) {
        usernameDisplay.textContent = 'Player: ' + username;
    }

    const usernameInput = document.getElementById('settings-username');
    if (usernameInput) {
        usernameInput.value = username;
    }

    const memory = localStorage.getItem('memory') || '4096';
    const memoryInput = document.getElementById('settings-memory');
    if (memoryInput) {
        memoryInput.value = memory;
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
        showToast(`Theme: ${theme === 'dark' ? '🌙 Dark' : '☀️ Light'}`, 'success', 2000);
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
        showToast(`Akzentfarbe geändert!`, 'success', 2000);
    }
}

const saveSettingsBtn = document.getElementById('save-settings-btn');
if (saveSettingsBtn) {
    saveSettingsBtn.addEventListener('click', () => {
        const usernameInput = document.getElementById('settings-username');
        const memoryInput = document.getElementById('settings-memory');

        if (usernameInput && memoryInput) {
            const username = usernameInput.value;
            const memory = memoryInput.value;

            localStorage.setItem('username', username);
            localStorage.setItem('memory', memory);

            currentUsername = username;
            const usernameDisplay = document.getElementById('username-display');
            if (usernameDisplay) {
                usernameDisplay.textContent = 'Player: ' + username;
            }

            showToast('Einstellungen gespeichert!', 'success', 3000);
        }
    });
}

// ==================== ACCOUNT MANAGEMENT ====================

let activeAccount = null;
let recentSkins = JSON.parse(localStorage.getItem('recentSkins') || '[]');

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
        if (typeEl) typeEl.textContent = activeAccount.is_microsoft ? '🔐 Microsoft' : '👤 Offline';
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
                    <img src="${activeAccount.head_url}" style="width: 64px; height: 64px; border-radius: 8px; image-rendering: pixelated;">
                    <div style="flex: 1;">
                        <p style="margin: 0; color: var(--text-primary); font-weight: bold; font-size: 18px;">${activeAccount.username}</p>
                        <p style="margin: 5px 0 0 0; color: var(--text-secondary); font-size: 12px;">
                            ${activeAccount.is_microsoft ? '🔐 Microsoft Account' : '👤 Offline Account'}
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
                    <div style="font-size: 32px; margin-bottom: 10px;">👤</div>
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
                        <img src="${acc.head_url}" style="width: 32px; height: 32px; border-radius: 4px; image-rendering: pixelated;">
                        <span style="flex: 1; color: var(--text-primary);">${acc.username}</span>
                        <span style="color: var(--text-secondary); font-size: 11px;">${acc.is_microsoft ? '🔐' : '👤'}</span>
                        <button class="btn btn-secondary" onclick="switchAccount('${acc.uuid}')" style="padding: 5px 10px; font-size: 11px;">
                            Wechseln
                        </button>
                        <button class="btn btn-secondary" onclick="logoutAccount('${acc.uuid}')" style="padding: 5px 10px; font-size: 11px; color: #f44336;">
                            ✕
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
                    <h3 style="color: var(--gold); margin: 0 0 25px 0;">🔐 Microsoft Login</h3>
                    
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
                            📋 Code kopieren
                        </button>
                    </div>
                    
                    <div id="login-status" style="margin-bottom: 20px;">
                        <div class="spinner" style="margin: 0 auto 10px;"></div>
                        <p style="color: var(--text-secondary); margin: 0;">Warte auf Anmeldung...</p>
                    </div>
                    
                    <button class="btn btn-secondary" onclick="cancelMicrosoftLogin()" style="padding: 10px 30px;">
                        Abbrechen
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
                <h3 style="color: var(--gold); margin: 0 0 20px 0;">👤 Offline Account erstellen</h3>
                <div style="margin-bottom: 20px;">
                    <label style="display: block; margin-bottom: 8px; color: var(--text-secondary);">Spielername</label>
                    <input type="text" id="offline-username" placeholder="Dein Minecraft-Name" 
                           style="width: 100%; padding: 12px; background: var(--bg-light); border: 2px solid var(--bg-light); border-radius: 8px; color: var(--text-primary);"
                           maxlength="16">
                    <p style="color: var(--text-secondary); font-size: 11px; margin-top: 5px;">
                        ⚠️ Offline-Accounts können nur auf Servern mit deaktivierter Online-Authentifizierung spielen
                    </p>
                </div>
                <div style="display: flex; gap: 10px;">
                    <button class="btn btn-secondary" onclick="document.getElementById('offline-account-modal').remove()" style="flex: 1;">
                        Abbrechen
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
    if (!confirm('Möchtest du diesen Account wirklich abmelden?')) return;

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

// ==================== SKIN VIEWER ====================

function updateSkinPage() {
    const render = document.getElementById('skin-3d-render');
    const playerName = document.getElementById('skin-player-name');
    const currentHead = document.getElementById('current-skin-head');
    const currentName = document.getElementById('current-skin-name');

    if (activeAccount) {
        const uuid = activeAccount.uuid;
        if (render) render.src = `https://mc-heads.net/body/${uuid}/150`;
        if (playerName) playerName.textContent = activeAccount.username;
        if (currentHead) currentHead.src = `https://mc-heads.net/avatar/${uuid}/64`;
        if (currentName) currentName.textContent = activeAccount.username;
    }

    renderRecentSkins();
}

async function searchPlayerSkin() {
    const input = document.getElementById('skin-search-input');
    const playerName = input.value.trim();

    if (!playerName) {
        showToast('Bitte gib einen Spielernamen ein', 'warning', 3000);
        return;
    }

    try {
        // Mojang API um UUID zu bekommen
        const response = await fetch(`https://api.mojang.com/users/profiles/minecraft/${playerName}`);

        if (!response.ok) {
            showToast('Spieler nicht gefunden', 'error', 3000);
            return;
        }

        const data = await response.json();
        const uuid = data.id;

        // Skin anzeigen
        const render = document.getElementById('skin-3d-render');
        const nameDisplay = document.getElementById('skin-player-name');

        if (render) render.src = `https://mc-heads.net/body/${uuid}/150`;
        if (nameDisplay) nameDisplay.textContent = data.name;

        // Zu "Zuletzt angesehen" hinzufügen
        addToRecentSkins(data.name, uuid);

        showToast(`Skin von ${data.name} geladen!`, 'success', 2000);
        input.value = '';

    } catch (error) {
        showToast('Fehler beim Laden des Skins', 'error', 3000);
    }
}

function addToRecentSkins(name, uuid) {
    // Entferne wenn bereits vorhanden
    recentSkins = recentSkins.filter(s => s.uuid !== uuid);

    // Am Anfang hinzufügen
    recentSkins.unshift({ name, uuid });

    // Max 12 Skins
    if (recentSkins.length > 12) {
        recentSkins = recentSkins.slice(0, 12);
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

    container.innerHTML = recentSkins.map(skin => `
        <div onclick="showSkin('${skin.uuid}', '${skin.name}')" 
             style="cursor: pointer; text-align: center; transition: transform 0.2s;"
             onmouseover="this.style.transform='scale(1.1)'" 
             onmouseout="this.style.transform='scale(1)'">
            <img src="https://mc-heads.net/avatar/${skin.uuid}/48" 
                 style="width: 48px; height: 48px; border-radius: 6px; image-rendering: pixelated;">
            <p style="color: var(--text-secondary); font-size: 10px; margin: 3px 0 0 0; max-width: 48px; overflow: hidden; text-overflow: ellipsis;">${skin.name}</p>
        </div>
    `).join('');
}

function showSkin(uuid, name) {
    const render = document.getElementById('skin-3d-render');
    const nameDisplay = document.getElementById('skin-player-name');

    if (render) render.src = `https://mc-heads.net/body/${uuid}/150`;
    if (nameDisplay) nameDisplay.textContent = name;
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

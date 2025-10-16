/**
 * Rune Editor - Client-side JavaScript for live markdown editing
 * 
 * This module provides the client-side functionality for the Rune markdown editor,
 * including mode switching, live editing, keyboard shortcuts, and WebSocket communication.
 */

// Editor configuration
const EDITOR_CONFIG = {
    AUTO_SAVE_DELAY: 2000, // 2 seconds
    RECONNECT_DELAY: 3000, // 3 seconds
    RENDER_DEBOUNCE: 300,  // 300ms
};

// Editor state (initialized in template.html)
// editorState is defined globally in template.html

/**
 * Live editor integration for WYSIWYG editing
 */
class LiveEditorIntegration {
    constructor() {
        this.activeElement = null;
        this.renderDebounceTimer = null;
    }

    /**
     * Handle click-to-edit functionality
     */
    handleClick(event) {
        const target = event.target;
        
        // Check if clicked element is editable
        if (this.isEditableElement(target)) {
            this.activateElement(target);
        }
    }

    /**
     * Check if element can be edited
     */
    isEditableElement(element) {
        // Check if element is a markdown element (heading, paragraph, list item, etc.)
        const editableTags = ['H1', 'H2', 'H3', 'H4', 'H5', 'H6', 'P', 'LI', 'BLOCKQUOTE'];
        return editableTags.includes(element.tagName);
    }

    /**
     * Activate element for editing
     */
    activateElement(element) {
        // Deactivate previous element
        if (this.activeElement) {
            this.deactivateElement(this.activeElement);
        }

        // Mark element as editing
        element.classList.add('editable-element', 'editing');
        element.contentEditable = 'true';
        element.focus();

        this.activeElement = element;

        // Add event listeners
        element.addEventListener('blur', () => this.deactivateElement(element));
        element.addEventListener('input', () => this.handleElementInput(element));
    }

    /**
     * Deactivate element editing
     */
    deactivateElement(element) {
        element.classList.remove('editing');
        element.contentEditable = 'false';
        
        if (this.activeElement === element) {
            this.activeElement = null;
        }
    }

    /**
     * Handle element input
     */
    handleElementInput(element) {
        setDirty(true);
        
        // Debounce rendering
        clearTimeout(this.renderDebounceTimer);
        this.renderDebounceTimer = setTimeout(() => {
            this.triggerRender();
        }, EDITOR_CONFIG.RENDER_DEBOUNCE);
    }

    /**
     * Trigger rendering
     */
    triggerRender() {
        if (editorState.isConnected && editorState.sessionId) {
            sendEditorMessage({
                type: 'trigger_render',
                session_id: editorState.sessionId,
                trigger_events: ['content_change']
            });
        }
    }

    /**
     * Initialize live editor
     */
    initialize() {
        const liveContent = document.getElementById('live-content');
        if (liveContent) {
            // Add click handler for click-to-edit
            liveContent.addEventListener('click', (e) => this.handleClick(e));
        }
    }
}

/**
 * Cursor position manager for tracking cursor across modes
 */
class CursorPositionManager {
    constructor() {
        this.rawPosition = 0;
        this.renderedPosition = null;
    }

    /**
     * Get current cursor position in raw mode
     */
    getRawPosition() {
        const textarea = document.getElementById('raw-textarea');
        if (textarea) {
            return textarea.selectionStart;
        }
        return 0;
    }

    /**
     * Set cursor position in raw mode
     */
    setRawPosition(position) {
        const textarea = document.getElementById('raw-textarea');
        if (textarea) {
            textarea.selectionStart = position;
            textarea.selectionEnd = position;
            textarea.focus();
        }
    }

    /**
     * Save current cursor position
     */
    save() {
        if (editorState.mode === 'raw') {
            this.rawPosition = this.getRawPosition();
        }
    }

    /**
     * Restore cursor position
     */
    restore() {
        if (editorState.mode === 'raw') {
            this.setRawPosition(this.rawPosition);
        }
    }
}

/**
 * Keyboard shortcut handler
 */
class KeyboardShortcutHandler {
    constructor() {
        this.shortcuts = new Map();
        this.registerDefaultShortcuts();
    }

    /**
     * Register default shortcuts
     */
    registerDefaultShortcuts() {
        // Save
        this.register('Ctrl+S', () => saveContent());
        this.register('Cmd+S', () => saveContent());

        // Mode switching
        this.register('Ctrl+E', () => this.toggleRawLive());
        this.register('Cmd+E', () => this.toggleRawLive());
        
        this.register('Ctrl+/', () => switchEditorMode('preview'));
        this.register('Cmd+/', () => switchEditorMode('preview'));

        // Formatting
        this.register('Ctrl+B', () => applyMarkdownFormat('bold'));
        this.register('Cmd+B', () => applyMarkdownFormat('bold'));
        
        this.register('Ctrl+I', () => applyMarkdownFormat('italic'));
        this.register('Cmd+I', () => applyMarkdownFormat('italic'));

        // Theme
        this.register('Ctrl+T', () => openThemeModal());
        this.register('Cmd+T', () => openThemeModal());

        // Help
        this.register('?', () => toggleShortcutsHelp());
        
        // Exit
        this.register('Escape', () => this.handleEscape());
    }

    /**
     * Register a shortcut
     */
    register(key, handler) {
        this.shortcuts.set(key.toLowerCase(), handler);
    }

    /**
     * Handle keyboard event
     */
    handle(event) {
        const isMac = navigator.platform.toUpperCase().indexOf('MAC') >= 0;
        const modifier = isMac ? event.metaKey : event.ctrlKey;
        
        let shortcutKey = '';
        
        if (modifier) {
            shortcutKey = (isMac ? 'Cmd+' : 'Ctrl+') + event.key;
        } else if (event.key === 'Escape' || event.key === '?') {
            shortcutKey = event.key;
        } else {
            return false;
        }

        const handler = this.shortcuts.get(shortcutKey.toLowerCase());
        if (handler) {
            event.preventDefault();
            handler();
            return true;
        }

        return false;
    }

    /**
     * Toggle between raw and live mode
     */
    toggleRawLive() {
        if (editorState.mode === 'raw') {
            switchEditorMode('live');
        } else if (editorState.mode === 'live') {
            switchEditorMode('raw');
        } else {
            switchEditorMode('raw');
        }
    }

    /**
     * Handle Escape key
     */
    handleEscape() {
        // Close shortcuts help if open
        const shortcutsOverlay = document.getElementById('shortcuts-overlay');
        if (shortcutsOverlay && shortcutsOverlay.classList.contains('show')) {
            shortcutsOverlay.classList.remove('show');
        } else {
            // Exit editor mode
            exitEditorMode();
        }
    }
}

/**
 * Syntax highlighter for raw mode
 */
class SyntaxHighlighter {
    constructor() {
        this.patterns = [
            { regex: /^#{1,6}\s.+$/gm, class: 'md-header' },
            { regex: /\*\*(.+?)\*\*/g, class: 'md-bold' },
            { regex: /\*(.+?)\*/g, class: 'md-italic' },
            { regex: /`(.+?)`/g, class: 'md-code' },
            { regex: /\[(.+?)\]\((.+?)\)/g, class: 'md-link' },
        ];
    }

    /**
     * Highlight syntax in text
     */
    highlight(text) {
        // This is a placeholder for syntax highlighting
        // In a real implementation, this would apply syntax highlighting
        // to the raw editor textarea
        return text;
    }
}

// Initialize editor components
let liveEditorIntegration;
let cursorPositionManager;
let keyboardShortcutHandler;
let syntaxHighlighter;

/**
 * Initialize all editor components
 */
function initializeEditorComponents() {
    liveEditorIntegration = new LiveEditorIntegration();
    cursorPositionManager = new CursorPositionManager();
    keyboardShortcutHandler = new KeyboardShortcutHandler();
    syntaxHighlighter = new SyntaxHighlighter();

    // Initialize live editor
    liveEditorIntegration.initialize();

    console.log('Editor components initialized');
}

/**
 * Enhanced mode switching with cursor preservation
 */
function enhancedSwitchMode(mode) {
    // Save cursor position before switching
    if (cursorPositionManager) {
        cursorPositionManager.save();
    }

    // Switch mode
    switchEditorMode(mode);

    // Restore cursor position after switching
    setTimeout(() => {
        if (cursorPositionManager) {
            cursorPositionManager.restore();
        }
    }, 100);
}

// Export functions for global use
window.initializeEditorComponents = initializeEditorComponents;
window.enhancedSwitchMode = enhancedSwitchMode;

// Initialize when DOM is ready
if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', initializeEditorComponents);
} else {
    initializeEditorComponents();
}

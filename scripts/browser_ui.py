"""User-facing dashboard workflows and responsive/offline checks."""
import json
from copy import deepcopy
from urllib.parse import urlsplit


def check_dashboard(page):
    page.bring_to_front()
    page.locator('#search').fill('demo-user')
    assert page.locator('.player').count() == 1
    assert page.locator('.card-name').inner_text() == 'demo-user'
    page.locator('#search').fill('no-such-player')
    assert page.locator('.player').count() == 0
    assert page.locator('.card-empty').count() == 2
    page.locator('#search').fill('')
    page.locator('#ctButton').click()
    assert page.locator('#ctPlayers .player').count() == 0
    assert page.locator('#tPlayers .player').count() > 0
    assert page.locator('#ctButton').get_attribute('aria-pressed') == 'false'
    page.locator('#ctButton').click()
    page.locator('#tButton').click()
    assert page.locator('#tPlayers .player').count() == 0
    assert page.locator('#ctPlayers .player').count() > 0
    page.locator('#tButton').click()
    page.locator('.player').first.click()
    assert page.locator('.player[aria-pressed=true]').count() == 1
    page.locator('.player').last.focus()
    page.keyboard.press('Enter')
    assert page.locator('.player').last.get_attribute('aria-pressed') == 'true'
    assert page.locator('.player').last.evaluate('e=>e===document.activeElement')
    page.locator('#namesBtn').click()
    assert page.locator('#namesBtn').get_attribute('aria-pressed') == 'true'
    assert page.evaluate('showNames')
    page.locator('#namesBtn').click()

    page.locator('#resetView').click()
    page.locator('#zoomIn').click()
    assert page.evaluate('scale') > 1
    page.locator('#zoomOut').click()
    assert abs(page.evaluate('scale') - 1) < .001
    canvas = page.locator('#radar').bounding_box()
    x, y = canvas['x'] + canvas['width'] * .75, canvas['y'] + canvas['height'] * .7
    page.mouse.move(x, y)
    page.mouse.wheel(0, -200)
    page.wait_for_function('()=>scale>1')
    page.locator('#resetView').click()
    page.mouse.move(x, y)
    page.mouse.down()
    page.mouse.move(x - 60, y - 40, steps=5)
    page.mouse.up()
    assert abs(page.evaluate('panX') + 60) < 1
    assert abs(page.evaluate('panY') + 40) < 1
    page.mouse.move(x, y)
    page.mouse.down()
    assert page.evaluate('dragging')
    page.locator('#radar').dispatch_event('pointercancel')
    assert not page.evaluate('dragging')
    page.mouse.up()
    page.locator('#resetView').click()
    assert page.evaluate('scale===1&&panX===0&&panY===0')
    page.wait_for_function("()=>!$('mapScale').hidden&&$('mapScaleLabel').textContent.includes('м')")

    origin = urlsplit(page.url)
    page.context.grant_permissions(['clipboard-read', 'clipboard-write'], origin=f'{origin.scheme}://{origin.netloc}')
    page.locator('#diagBtn').click()
    page.wait_for_function("()=>$('diagBtn').textContent==='COPIED'")
    data = json.loads(page.evaluate('navigator.clipboard.readText()'))
    assert data['game_map'] and 'diagnostics' in data and 'token=' not in json.dumps(data)
    print('PASS: search, empty results, both team filters, mouse/keyboard selection, names, zoom, wheel, drag, reset, ruler and DIAG clipboard')


def check_preferences(page):
    page.locator('#settingsBtn').click()
    page.locator('#settingsTabMap').click()
    original = page.evaluate('JSON.parse(JSON.stringify(prefs))')
    changed = 0
    controls = page.locator('#settingsMap [data-pref-field]').all()
    for control in controls:
        field = control.get_attribute('data-pref-field')
        kind = control.get_attribute('type')
        if kind == 'checkbox':
            control.set_checked(not original[field])
            expected = not original[field]
        elif kind == 'color':
            expected = '#aabbcc'
            control.evaluate("(e,v)=>{e.value=v;e.dispatchEvent(new Event('input',{bubbles:true}));}", expected)
        else:
            expected = 15 if original[field] != 15 else 60
            control.evaluate("(e,v)=>{e.value=v;e.dispatchEvent(new Event('input',{bubbles:true}));}", str(expected))
        saved = page.evaluate("JSON.parse(localStorage.getItem('cas-radar-v06-settings'))")
        assert saved[field] == expected, f'{field} was not saved'
        changed += 1
    page.locator('#settingsTabSound').click()
    for control in page.locator('#settingsSound [data-pref-field]').all():
        field = control.get_attribute('data-pref-field')
        kind = control.get_attribute('type')
        if kind == 'checkbox':
            expected = not original[field]
            control.set_checked(expected)
        else:
            expected = '#aabbcc' if kind == 'color' else 10
            control.evaluate("(e,v)=>{e.value=String(v);e.dispatchEvent(new Event('input',{bubbles:true}));}", expected)
        saved = page.evaluate("JSON.parse(localStorage.getItem('cas-radar-v06-settings'))")
        assert saved[field] == expected, f'{field} was not saved'
        changed += 1
    page.locator('#saveSettings').click()
    page.reload()
    page.wait_for_selector('.player')
    page.locator('#settingsBtn').click()
    page.locator('#settingsTabMap').click()
    assert page.locator('#friendColor').input_value() == '#aabbcc'
    assert page.locator('#showDead').is_checked() != original['showDead']
    page.locator('#resetSettings').click()
    assert page.locator('#friendColor').input_value() == '#54b8ff'
    assert not page.locator('#showDead').is_checked()
    assert page.locator('#maxFps').input_value() == '30'
    page.locator('#settingsTabSound').click()
    assert page.locator('#noiseColor').input_value() == '#aabbcc', 'Map reset changed sound preferences'
    page.locator('#resetSettings').click()
    assert page.locator('#showNoise').is_checked()
    assert page.locator('#noiseOpacity').input_value() == '75'
    page.locator('#settingsTabMap').click()
    page.locator('#settingsTabMap').focus()
    page.keyboard.press('ArrowRight')
    assert page.locator('#settingsTabMini').get_attribute('aria-selected') == 'true'
    page.keyboard.press('End')
    assert page.locator('#settingsTabSound').get_attribute('aria-selected') == 'true'
    page.keyboard.press('Home')
    assert page.locator('#settingsTabMap').get_attribute('aria-selected') == 'true'
    page.keyboard.press('Escape')
    assert page.locator('#settingsBackdrop').is_hidden()
    assert page.locator('#settingsBtn').evaluate('e=>document.activeElement===e')
    assert not page.locator('.layout').evaluate('e=>e.inert')
    page.locator('#settingsBtn').click()
    assert page.locator('.layout').evaluate('e=>e.inert')
    page.locator('#closeSettings').click()
    assert page.locator('#settingsBackdrop').is_hidden()
    print(f'PASS: all {changed} map/sound preferences save, survive reload and reset; keyboard tabs, modal close, focus restoration and inert background')


def check_all_native_fields(page, saved):
    """Use the schema-backed Windows response set up by check_settings.

    Windows CI separately runs the real API and GDI window. This portable check
    verifies that every visible control sends its own setting without changing
    neighboring fields, including the inverted transparency slider.
    """
    seen = set()
    for tab, section in [('mini', 'players'), ('mini', 'objects'), ('mini', 'window'), ('sound', None)]:
        page.locator(f'[data-settings-tab="{tab}"]').click()
        if section:
            page.locator(f'[data-native-section="{section}"]').click()
        for control in page.locator('[data-native-field]:visible').all():
            field = control.get_attribute('data-native-field')
            if field in seen:
                continue
            seen.add(field)
            before = deepcopy(saved)
            original = control.input_value()
            kind = control.get_attribute('type')
            if kind == 'checkbox':
                expected = not control.is_checked()
                control.set_checked(expected)
            else:
                value = '#a1b2c3' if kind == 'color' else control.get_attribute('min')
                if value == original and kind == 'range':
                    value = control.get_attribute('max')
                control.evaluate("(e,v)=>{e.value=v;e.dispatchEvent(new Event('change',{bubbles:true}));}", value)
                expected = value if kind == 'color' else float(value)
                if control.get_attribute('data-invert') == 'true':
                    expected = 100 - expected
            page.evaluate('nativePending')
            assert saved[field] == expected, f'Wrong value sent for {field}'
            assert all(saved[key] == value for key, value in before.items() if key != field), f'{field} changed another field'
            if kind == 'checkbox':
                control.set_checked(before[field])
            else:
                control.evaluate("(e,v)=>{e.value=v;e.dispatchEvent(new Event('change',{bubbles:true}));}", original)
            page.evaluate('nativePending')
            assert saved == before, f'{field} did not restore'
    expected_fields = set(page.locator('[data-native-field]').evaluate_all('els=>els.map(e=>e.dataset.nativeField)'))
    assert seen == expected_fields, f'Controls never reached: {expected_fields - seen}'
    page.locator('#settingsTabMini').click()
    page.locator('[data-native-section="players"]').click()
    print(f'PASS: all {len(seen)} native settings controls send the correct field/value and leave other settings intact')


def check_responsive_offline(browser, url, screenshots=None):
    context = browser.new_context(viewport={'width':1440,'height':920})
    page = context.new_page()
    errors = []
    page.on('pageerror', lambda e: errors.append(str(e)))
    # The app must render with every external resource blocked.
    def local_only(route):
        if urlsplit(route.request.url).hostname == '127.0.0.1':
            route.continue_()
        else:
            route.abort()
    page.route('**/*', local_only)
    try:
        page.goto(url)
        page.wait_for_selector('.player')
        page.evaluate('document.fonts.ready')
        assert page.evaluate("[...document.fonts].length===3&&[...document.fonts].every(f=>f.status==='loaded')")
        for width, height in [(320,568),(390,844),(768,1024),(1024,768),(1440,920),(1920,1080)]:
            page.set_viewport_size({'width':width,'height':height})
            assert page.evaluate('document.documentElement.scrollWidth<=innerWidth'), f'Horizontal overflow at {width}'
            assert page.locator('#radar').bounding_box()['height'] >= 200
            page.locator('#settingsBtn').click()
            box = page.locator('.settings-box').bounding_box()
            assert box['x'] >= 0 and box['x'] + box['width'] <= width
            assert box['y'] >= 0 and box['y'] + box['height'] <= height
            assert page.locator('.settings-body').evaluate('e=>e.scrollWidth<=e.clientWidth+1')
            assert page.locator('#saveSettings').is_visible()
            page.keyboard.press('Escape')
            if screenshots and width in (390,1440):
                screenshots.mkdir(parents=True, exist_ok=True)
                page.screenshot(path=str(screenshots/f'dashboard-{width}.png'),full_page=True)
        page.locator('#miniBtn').click()
        response = page.evaluate("async()=>(await fetch(api('/api/overlay'))).json()")
        if not response['supported']:
            page.wait_for_function("()=>$('overlayStatus').textContent==='Требуется Windows'")
            assert all(control.is_disabled() for control in page.locator('[data-native-field]').all())
        assert not errors, errors
        print('PASS: six viewport sizes (320–1920 px), usable settings, bundled offline fonts, and explicit Windows-only availability')
    finally:
        context.close()

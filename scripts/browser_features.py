"""Sound-wave and settings checks used by browser-smoke.py."""
import time
from copy import deepcopy
from browser_ui import check_all_native_fields


def check_steam_avatars(page):
    state = page.evaluate("JSON.parse(JSON.stringify(snapshot))")
    first, second = "76561197960287930", "76561198000000001"
    for player in state["players"]:
        player["steam_id"] = None
        player["agent_model"] = "characters/models/tm_professional/tm_professional_varf5.vmdl"
    state["players"][0]["steam_id"] = first
    state["players"][1]["steam_id"] = second
    player_id = state["players"][0]["id"]

    def avatars_state(route):
        state["time"] = time.time()
        route.fulfill(json=state)

    page.route("**/api/state?*", avatars_state)
    try:
        page.wait_for_function("() => document.querySelectorAll('.steam-avatar').length===2")
        page.wait_for_function("() => [...document.querySelectorAll('.steam-avatar')].every(img=>img.complete&&img.naturalWidth>0)")
        ids = page.locator(".steam-avatar").evaluate_all("images=>images.map(img=>new URL(img.src).searchParams.get('steam_id'))")
        assert ids == [first, second], "SteamID64 was rounded or replaced by an agent model"
        assert page.locator('.agent-portrait, img[src*="/api/agent-icon"]').count() == 0
        assert page.locator(".card-avatar:not(:has(img))").count() > 0, "Missing Steam avatars must keep a neutral silhouette"
        state["players"][0]["steam_id"] = second
        page.wait_for_function("([id,steamId]) => new URL(document.querySelector(`.player[data-id='${id}'] .steam-avatar`).src).searchParams.get('steam_id')===steamId", arg=[player_id, second])

        def broken_avatar(route):
            route.abort()

        page.route("**/api/steam-avatar?steam_id=76561198000000003&*", broken_avatar)
        state["players"][0]["steam_id"] = "76561198000000003"
        page.wait_for_function("id => document.querySelector(`.player[data-id='${id}'] .steam-avatar`)?.hidden===true", arg=player_id)
        print("PASS: player cards load exact Steam avatars, update reused slots and keep neutral fallbacks")
    finally:
        page.unroute("**/api/state?*", avatars_state)
        page.unroute("**/api/steam-avatar?steam_id=76561198000000003&*")


def check_noise(page):
    page.bring_to_front()
    page.locator("#layerSelect").select_option("default")
    state = page.evaluate("JSON.parse(JSON.stringify(snapshot))")
    local = next(player for player in state["players"] if player["local"])
    origin = [local["position"][0] - 150, local["position"][1] + 180, local["position"][2]]
    started = None

    def sound_state(route):
        nonlocal started
        if started is None:
            started = time.monotonic()
        age = int((time.monotonic() - started) * 1000)
        value = deepcopy(state)
        value.update(time=time.time(), age_secs=0, status="live", grenades=[], noise=None)
        value["noise_events"] = [] if age >= 2000 else [{
            "event_id": 900001, "kind": "footstep", "position": origin,
            "radius_units": 1100, "age_ms": age, "duration_ms": 2000, "approximate": True,
        }]
        next(player for player in value["players"] if player["local"])["position"][0] += age / 4
        route.fulfill(json=value)

    page.evaluate("""origin => {
        window.auditNoise=[];window.auditNoiseLabels=[];
        window.auditNoiseOrigin=origin;
        window.auditOriginalArc=ctx.arc;window.auditOriginalText=ctx.fillText;
        ctx.arc=function(x,y,r,...rest){
            const origin=xy(window.auditNoiseOrigin);
            if(ctx.lineWidth===2&&ctx.strokeStyle===prefs.noiseColor.toLowerCase()&&Math.hypot(x-origin.x,y-origin.y)<.1)window.auditNoise.push({x,y,r,alpha:ctx.globalAlpha});
            return window.auditOriginalArc.call(ctx,x,y,r,...rest);
        };
        ctx.fillText=function(text,...rest){
            if(String(text).includes('28 м'))window.auditNoiseLabels.push(text);
            return window.auditOriginalText.call(ctx,text,...rest);
        };
    }""", origin)
    page.route("**/api/state?*", sound_state)
    try:
        page.wait_for_function("() => snapshot.noise_events?.some(n=>n.event_id===900001)")
        page.wait_for_function("() => window.auditNoise.length>2")
        page.wait_for_timeout(300)
        samples = page.evaluate("window.auditNoise")
        assert samples[-1]["r"] > samples[0]["r"], "Sound wave did not expand"
        assert samples[-1]["alpha"] < samples[0]["alpha"], "Sound wave did not fade"
        expected = page.evaluate("() => xy(snapshot.noise_events[0].position)")
        assert all(abs(sample["x"] - expected["x"]) < .1 and abs(sample["y"] - expected["y"]) < .1 for sample in samples)
        assert page.evaluate("window.auditNoiseLabels.length") > 0
        page.wait_for_function("() => snapshot.noise_events.length===0", timeout=4000)
        before = page.evaluate("window.auditNoise.length")
        page.wait_for_timeout(100)
        assert page.evaluate("window.auditNoise.length") == before
        print("PASS: sound waves grow, fade, remain at the emission origin and show the model distance")
    finally:
        page.unroute("**/api/state?*", sound_state)
        page.evaluate("() => {ctx.arc=window.auditOriginalArc;ctx.fillText=window.auditOriginalText;}")


def check_settings(page, screenshots=None):
    # Linux has no native window. Exercise the browser controls with the real
    # settings schema and an isolated Windows-capable response. Windows CI
    # separately verifies the real API, native rendering and disk persistence.
    response = page.evaluate("async () => (await fetch(api('/api/overlay'))).json()")
    saved = deepcopy(response["settings"])
    defaults = deepcopy(response["defaults"])

    def overlay(route):
        if route.request.method == "POST":
            saved.update(route.request.post_data_json)
        route.fulfill(json={"supported": True, "window_ready": True, "map_ready": True,
                            "settings": saved, "defaults": defaults})

    def saved_ui():
        page.wait_for_function("() => $('overlayStatus').textContent.startsWith('Сохранено')")

    def set_transparency(value):
        page.locator("#nativeOpacity").evaluate("""(element,value)=>{
            element.value=String(value);element.dispatchEvent(new Event('input',{bubbles:true}));
            element.dispatchEvent(new Event('change',{bubbles:true}));
        }""", value)
        saved_ui()

    page.route("**/api/overlay?*", overlay)
    try:
        page.locator("#miniBtn").click()
        page.wait_for_function("() => nativeAvailable&&!$('nativeHP').disabled")
        assert page.locator("#nativeOpacity").is_visible(), "Transparency must be visible without selecting a subgroup"
        assert page.locator("#nativeOpacityLabel").inner_text() == "0%"
        for transparency in (90, 0, 43):
            set_transparency(transparency)
            assert saved["opacity"] == 100 - transparency, "Transparency was saved as opacity without inversion"
            assert page.locator("#nativeOpacityLabel").inner_text() == f"{transparency}%"
        page.locator("#nativeHP").uncheck()
        page.locator("#nativeWeaponName").uncheck()
        saved_ui()
        assert not saved["show_hp"] and not saved["show_weapon_name"]
        page.locator('[data-native-preset="compact"]').click()
        saved_ui()
        assert saved["show_hp"] and not saved["show_hp_bar"] and not saved["show_weapon_name"]
        page.locator('[data-native-preset="detailed"]').click()
        saved_ui()
        assert saved["show_names"] and saved["show_armor"] and saved["show_hp_bar"] and saved["show_weapon_name"]
        page.locator("#nativeHP").uncheck()
        saved_ui()
        page.locator('[data-native-section="objects"]').click()
        assert page.locator("#nativeOpacity").is_visible()
        page.locator("#nativeUtilityTimers").uncheck()
        saved_ui()
        assert not saved["show_utility_timers"]
        page.locator('[data-native-section="window"]').click()
        assert page.locator("#nativeOpacity").is_visible()
        page.locator("#nativeMaxFps").evaluate("element=>{element.value='15';element.dispatchEvent(new Event('change',{bubbles:true}));}")
        saved_ui()
        assert saved["max_fps"] == 15
        page.locator("#nativeVisible").check()
        page.locator("#nativeZoom").evaluate("element=>{element.value='1.5';element.dispatchEvent(new Event('input',{bubbles:true}));element.dispatchEvent(new Event('change',{bubbles:true}));}")
        saved_ui()
        assert saved["visible"] and saved["zoom"] == 1.5
        page.locator("#settingsTabSound").click()
        page.locator("#showNoise").uncheck()
        page.locator("#nativeNoiseAnimation").uncheck()
        saved_ui()
        assert not saved["noise_animation"]
        page.locator("#saveSettings").click()

        page.reload()
        page.wait_for_selector(".player")
        page.locator("#settingsBtn").click()
        page.locator("#settingsTabSound").click()
        page.wait_for_function("() => nativeAvailable&&!$('nativeNoiseAnimation').disabled")
        assert not page.locator("#showNoise").is_checked()
        assert not page.locator("#nativeNoiseAnimation").is_checked()
        page.locator("#resetSettings").click()
        saved_ui()
        assert page.locator("#showNoise").is_checked()
        assert saved["noise_animation"] and not saved["show_hp"], "Sound reset changed player settings"
        assert saved["opacity"] == 57, "Sound reset changed window transparency"
        page.locator("#settingsTabMini").click()
        assert page.locator("#nativeOpacity").input_value() == "43", "Transparency was not restored after reload"
        assert page.locator("#nativeOpacityLabel").inner_text() == "43%"
        page.locator('[data-native-section="window"]').click()
        page.locator("#resetSettings").click()
        saved_ui()
        assert saved["opacity"] == defaults["opacity"] and saved["zoom"] == defaults["zoom"]
        assert saved["max_fps"] == defaults["max_fps"]
        assert not saved["show_hp"], "Window reset changed player settings"
        assert page.locator("#nativeOpacityLabel").inner_text() == "0%"
        set_transparency(43)
        page.locator('[data-native-section="players"]').click()
        assert not page.locator("#nativeHP").is_checked()
        assert page.locator("#nativeNames").is_checked()
        check_all_native_fields(page, saved)
        if screenshots:
            screenshots.mkdir(parents=True, exist_ok=True)
            page.screenshot(path=str(screenshots / "mini-settings-desktop.png"))
        page.set_viewport_size({"width": 390, "height": 844})
        page.wait_for_timeout(100)
        assert page.locator(".settings-box").evaluate("element=>element.scrollWidth<=element.clientWidth+1")
        assert page.locator(".settings-footer").evaluate("element=>element.getBoundingClientRect().bottom<=innerHeight")
        page.locator("#saveSettings").focus()
        page.keyboard.press("Tab")
        assert page.locator("#closeSettings").evaluate("element=>document.activeElement===element")
        if screenshots:
            page.screenshot(path=str(screenshots / "mini-settings-mobile.png"))
        page.keyboard.press("Escape")
        assert page.locator("#settingsBackdrop").is_hidden()
        page.set_viewport_size({"width": 1512, "height": 982})
        print("PASS: detailed mini-map controls, visible transparency, presets, save/reload, scoped reset, mobile layout and keyboard navigation")
    finally:
        page.unroute("**/api/overlay?*", overlay)


def check_bomb_age(page):
    value = page.evaluate("JSON.parse(JSON.stringify(snapshot))")
    value.update(time=value["time"] + 1, age_secs=1.2, status="live")
    value["bomb_timer"] = {"site": "B", "remaining_secs": 20, "total_secs": 40, "being_defused": True}
    keep_fresh = True

    def bomb_state(route):
        if keep_fresh:
            value["time"] += .01
        route.fulfill(json=value)

    page.route("**/api/state?*", bomb_state)
    try:
        page.wait_for_function("() => snapshot.age_secs===1.2&&$('bombTimerSite').textContent==='B'")
        keep_fresh = False
        page.wait_for_function("expected=>snapshot.time===expected", arg=value["time"])
        initial = float(page.locator("#bombTimerSecs").inner_text())
        assert 18 < initial <= 18.8, "Timer ignored source age or subtracted it twice"
        page.wait_for_timeout(200)
        current = float(page.locator("#bombTimerSecs").inner_text())
        assert current < initial, "Repeated delivery of one snapshot reset the countdown"
        value.pop("bomb_timer")
        value.update(time=value["time"] + 1, age_secs=0)
        page.wait_for_function("() => !snapshot.bomb_timer&&$('bombTimerPanel').hidden")
        print("PASS: C4 accounts for source age, keeps counting on repeated samples, and clears on defuse/removal")
    except Exception:
        print("C4 test state:",page.evaluate("() => ({status:snapshot.status,age:snapshotAge(),sourceAge:snapshot.age_secs,bomb:snapshot.bomb_timer,hidden:document.hidden,enabled:prefs.showBombTimer,pollRunning,connectionError})"))
        raise
    finally:
        page.unroute("**/api/state?*", bomb_state)


def check_render_budget(browser, url):
    # Use a fresh, unrouted context: Playwright routing disables the HTTP cache.
    context = browser.new_context(viewport={"width": 1512, "height": 982})
    page = context.new_page()
    try:
        page.goto(url)
        page.wait_for_selector(".player")
        page.evaluate("document.querySelectorAll('img').forEach(image=>image.loading='eager')")
        page.wait_for_function("() => [...document.querySelectorAll('.equip-chip img,.card-primary img')].every(image=>image.complete)")
        page.evaluate("performance.clearResourceTimings()")
        for _ in range(3):
            page.evaluate("() => {selected=selected===null?1:null;repaintPlayers();}")
            page.wait_for_timeout(75)
        page.wait_for_timeout(200)
        assert page.evaluate("performance.getEntriesByType('resource').filter(e=>e.name.includes('/api/weapon-icon')&&e.transferSize>0).length") == 0, "Repainting cards downloaded cached icons again"

        page.locator("#settingsBtn").click()
        page.locator("#settingsTabMap").click()
        page.locator("#maxFps").evaluate("element=>{element.value='15';element.dispatchEvent(new Event('input',{bubbles:true}));}")
        page.locator("#saveSettings").click()
        page.evaluate("() => {window.auditPaints=0;const clear=ctx.clearRect.bind(ctx);ctx.clearRect=(...args)=>{window.auditPaints++;return clear(...args);};}")
        start = page.evaluate("performance.now()")
        page.wait_for_timeout(750)
        elapsed = (page.evaluate("performance.now()") - start) / 1000
        assert 0 < page.evaluate("window.auditPaints") <= 15 * elapsed + 2, "Renderer exceeded its saved frame budget"

        page.evaluate("() => {Object.defineProperty(document,'hidden',{configurable:true,value:true});document.dispatchEvent(new Event('visibilitychange'));}")
        page.wait_for_timeout(200)
        before = page.evaluate("({frames:window.auditPaints,requests:performance.getEntriesByType('resource').filter(e=>e.name.includes('/api/state')).length})")
        page.wait_for_timeout(400)
        after = page.evaluate("({frames:window.auditPaints,requests:performance.getEntriesByType('resource').filter(e=>e.name.includes('/api/state')).length})")
        assert before == after, "Hidden radar continued painting or polling"
        page.evaluate("() => {delete document.hidden;document.dispatchEvent(new Event('visibilitychange'));}")
        page.wait_for_function("count=>window.auditPaints>count", arg=before["frames"])
        page.wait_for_function("() => !connectionError&&!snapshotStale()")
        page.reload()
        page.wait_for_function("() => prefs.maxFps===15")
        print("PASS: cached equipment, saved FPS limit, hidden-tab suspension and visible-tab recovery")
    finally:
        context.close()

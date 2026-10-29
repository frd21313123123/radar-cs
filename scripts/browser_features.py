"""Sound-wave and settings checks used by browser-smoke.py."""
import time
from copy import deepcopy


def check_noise(page):
    page.locator("#layerSelect").select_option("default")
    state = page.evaluate("JSON.parse(JSON.stringify(snapshot))")
    local = next(player for player in state["players"] if player["local"])
    origin = [local["position"][0] - 150, local["position"][1] + 180, local["position"][2]]
    started = time.monotonic()

    def sound_state(route):
        age = int((time.monotonic() - started) * 1000)
        value = deepcopy(state)
        value.update(time=time.time(), status="live", grenades=[], noise=None)
        value["noise_events"] = [] if age >= 2000 else [{
            "event_id": 900001, "kind": "footstep", "position": origin,
            "radius_units": 1100, "age_ms": age, "duration_ms": 2000, "approximate": True,
        }]
        next(player for player in value["players"] if player["local"])["position"][0] += age / 4
        route.fulfill(json=value)

    page.route("**/api/state?*", sound_state)
    page.wait_for_function("() => snapshot.noise_events?.some(n=>n.event_id===900001)")
    page.evaluate("""() => {
        window.auditNoise=[];window.auditNoiseLabels=[];
        window.auditOriginalArc=ctx.arc;window.auditOriginalText=ctx.fillText;
        ctx.arc=function(x,y,r,...rest){
            if(ctx.shadowBlur===9&&ctx.lineWidth===2)window.auditNoise.push({x,y,r,alpha:ctx.globalAlpha});
            return window.auditOriginalArc.call(ctx,x,y,r,...rest);
        };
        ctx.fillText=function(text,...rest){
            if(String(text).includes('28 м'))window.auditNoiseLabels.push(text);
            return window.auditOriginalText.call(ctx,text,...rest);
        };
    }""")
    try:
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

    page.route("**/api/overlay?*", overlay)
    try:
        page.locator("#miniBtn").click()
        page.wait_for_function("() => nativeAvailable&&!$('nativeHP').disabled")
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
        page.locator("#nativeSectionSelect").select_option("objects")
        page.locator("#nativeUtilityTimers").uncheck()
        saved_ui()
        assert not saved["show_utility_timers"]
        page.locator("#nativeSectionSelect").select_option("window")
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
        page.locator("#settingsTabMini").click()
        page.locator("#nativeSectionSelect").select_option("players")
        assert not page.locator("#nativeHP").is_checked()
        assert page.locator("#nativeNames").is_checked()
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
        print("PASS: detailed mini-map controls, presets, save/reload, scoped reset, mobile layout and keyboard navigation")
    finally:
        page.unroute("**/api/overlay?*", overlay)

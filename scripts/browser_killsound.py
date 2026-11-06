"""Killsound UI against the real upload/settings API, with Windows capability
and preview mocked on Linux. Native playback needs a Windows audio device.
"""
from pathlib import Path
from urllib.parse import urlsplit, parse_qs


def check_killsound(page):
    token = parse_qs(urlsplit(page.url).query)['token'][0]
    parsed = urlsplit(page.url)
    origin = parsed.scheme + '://' + parsed.netloc
    endpoint = origin + '/api/killsound?token=' + token
    fixture = Path(__file__).parent / 'fixtures/killsound.mp3'
    payload = fixture.read_bytes()
    assert page.request.post(origin + '/api/killsound/file?name=test.mp3', data=payload,
                             headers={'Content-Type': 'audio/mpeg'}).status == 403
    rejected = page.request.post(origin + '/api/killsound/file?name=wrong.mp3&token=' + token,
                                 data=b'not an mp3', headers={'Content-Type': 'audio/mpeg'})
    assert rejected.status == 400
    assert page.request.post(endpoint, data={'enabled': True}).status == 400

    previews = []

    def windows_capability(route):
        if '/preview?' in route.request.url:
            previews.append(True)
            route.fulfill(json={**page.request.get(endpoint).json(), 'supported': True})
            return
        response = page.request.fetch(route.request)
        data = response.json()
        if response.ok:
            data['supported'] = True
        route.fulfill(status=response.status, json=data)

    page.route('**/api/killsound**', windows_capability)
    try:
        page.locator('#settingsBtn').click()
        page.locator('#settingsTabSound').click()
        page.wait_for_function("() => !$('killSoundFile').disabled")
        assert page.locator('#killSoundEnabled').is_disabled()
        page.locator('#killSoundFile').set_input_files({'name': 'Мой звук.mp3', 'mimeType': 'audio/mpeg', 'buffer': payload})
        page.wait_for_function("() => !killSoundBusy && killSound?.name === 'Мой звук.mp3'")
        page.locator('#killSoundEnabled').check()
        page.wait_for_function('() => !killSoundBusy && killSound?.enabled')
        page.locator('#killSoundVolume').evaluate("e => {e.value=42;e.dispatchEvent(new Event('change',{bubbles:true}));}")
        page.wait_for_function('() => !killSoundBusy && killSound?.volume === 42')
        page.locator('#killSoundPreview').click()
        page.wait_for_function('() => !killSoundBusy')
        assert previews
        saved = page.request.get(endpoint).json()
        assert saved['enabled'] and saved['volume'] == 42 and saved['name'] == 'Мой звук.mp3'
        page.reload()
        page.wait_for_function('() => killSound?.volume === 42 && killSound?.enabled')
        page.locator('#settingsBtn').click()
        page.locator('#settingsTabSound').click()
        page.locator('#resetSettings').click()
        page.wait_for_function('() => !killSoundBusy && !killSound.enabled && killSound.volume === 70')
        assert page.request.get(endpoint).json()['has_file'], 'Reset should retain the uploaded MP3'
        page.locator('#closeSettings').click()
        print('PASS: MP3 validation/authentication, upload, enable, volume, preview, reload and reset')
    finally:
        page.wait_for_timeout(100)
        page.unroute('**/api/killsound**', windows_capability)

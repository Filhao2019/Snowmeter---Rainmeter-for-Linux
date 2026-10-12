"""Point Mond's Weather skin at wttr.in (keyless JSON, location by IP) instead of the
dead Yahoo page. Keeps the file's UTF-16LE encoding and saves the original next to it."""
import re, shutil, sys
from pathlib import Path

ini = Path(sys.argv[1])
backup = ini.with_name("Weather.ini.yahoo-original")
if not backup.exists():
    shutil.copy2(ini, backup)
text = backup.read_bytes().decode("utf-16")

URL = "URL=https://wttr.in/?format=j1"
REGEXP = (r'RegExp=(?siU)"temp_F":\s*"(.*)".*"weatherCode":\s*"(.*)".*"weatherDesc":\s*\[\s*\{\s*"value":\s*"(.*)"'
          r'.*"nearest_area".*"areaName":\s*\[\s*\{\s*"value":\s*"(.*)"')
ICONS = ('RegExpSubstitute=1\r\nSubstitute="^113$":"clear_day","^116$":"partly_cloudy_day","^(119|122)$":"cloudy",'
         '"^(143|248|260)$":"fog","^(179|182|185|281|284|311|314|317|320|350|362|365|374|377)$":"Sleet",'
         '"^(227|230|323|326|329|332|335|338|368|371|392|395)$":"snow",'
         '"^(176|200|263|266|293|296|299|302|305|308|353|356|359|386|389)$":"rain"')

text = re.sub(r"(?m)^URL=https://www\.yahoo\.com/news/weather\s*$", URL, text)
text = re.sub(r"(?m)^RegExp=\(\?siU\)<h1.*$", lambda m: REGEXP, text)
# Yahoo's capture order was location, icon, condition, °F; wttr's is °F, code, condition, location.
new_index = {"MeasureLocation": 4, "MeasureWeatherIcons": 2, "MeasureWeatherCond": 3, "MeasureTempFahrenheit": 1}
for section, idx in new_index.items():
    text = re.sub(rf"(?s)(\[{section}\].*?^StringIndex=)\d+", rf"\g<1>{idx}", text, count=1, flags=re.M)
text = re.sub(r'(?s)(\[MeasureWeatherIcons\].*?)^Substitute=.*?$', lambda m: m.group(1) + ICONS, text, count=1, flags=re.M)
ini.write_bytes(text.encode("utf-16"))
print("patched", ini)

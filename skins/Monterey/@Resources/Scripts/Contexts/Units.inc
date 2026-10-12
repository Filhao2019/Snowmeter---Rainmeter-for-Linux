[Variables]
UnitsContextTitle=[#tCelsius]
UnitsContextAction=[!WriteKeyValue Variables "Units" "celsius" "#@#Variables\Global.inc"][!RefreshGroup Monterey]
UnitsContextTitle2=[#tFahrenheit]
UnitsContextAction2=[!WriteKeyValue Variables "Units" "fahrenheit" "#@#Variables\Global.inc"][!RefreshGroup Monterey]
UnitsContextTitle3=[#tKelvin]
UnitsContextAction3=[!WriteKeyValue Variables "Units" "kelvin" "#@#Variables\Global.inc"][!RefreshGroup Monterey]
UnitsContextTitle4=

[SetUnitsTickMark]
Measure=String
String=#Units#
DynamicVariables=1
IfMatch=celsius
IfMatchAction=[!SetVariable UnitsContextTitle "#tCelsius#  [\x2714]"]
IfMatch2=fahrenheit
IfMatchAction2=[!SetVariable UnitsContextTitle2 "#tFahrenheit#  [\x2714]"]
IfMatch3=kelvin
IfMatchAction3=[!SetVariable UnitsContextTitle3 "#tKelvin#  [\x2714]"]
[Variables]
WidgetHeight=400

[ThemeUpdaterShape]
Meter=Shape
Shape=Rectangle 20,20,(#WidgetWidth# - #SidebarWidth# - 40),40,6 | Fill Color #ForegroundColor#05 | StrokeWidth 0
X=#SidebarWidth#
Y=55
DynamicVariables=1

[ThemeUpdaterMode]
Meter=String
Text=#tThemeUpdater#
FontFace=#FontFace#
FontWeight=400
FontColor=#ForegroundColor#
FontSize=11
AntiAlias=1
X=(#SidebarWidth#+30)
Y=(85)
DynamicVariables=1

[ThemeUpdaterOpened]
Measure=Plugin
Plugin=ConfigActive
ConfigName=Monterey\Extras\ThemeUpdater
UpdateDivider=1
OnChangeAction=[!Refresh]

[ThemeUpdaterFalse]
Meter=Shape
Shape=Rectangle 0,0,38,24,12 | Fill Color 808080 | StrokeWidth 0
Shape2=Rectangle 2,2,20,20,10 | Fill Color ffffff | StrokeWidth 0
Y=(82)
X=(#WidgetWidth# - 30 - 40)
Hidden=([&ThemeUpdaterOpened:] <> -1)
DynamicVariables=1
LeftMouseUpAction=[!ToggleConfig "Monterey\Extras\ThemeUpdater" "ThemeUpdater.ini"]

[ThemeUpdaterTrue]
Meter=Shape
Shape=Rectangle 0,0,38,24,12 | Fill Color 33cc66 | StrokeWidth 0
Shape2=Rectangle 16,2,20,20,10 | Fill Color ffffff | StrokeWidth 0
Y=(82)
X=(#WidgetWidth# - 30 - 40)
Hidden=([&ThemeUpdaterOpened:] <> 1)
DynamicVariables=1
LeftMouseUpAction=[!ToggleConfig "Monterey\Extras\ThemeUpdater" "ThemeUpdater.ini"]

[ThemeUpdaterHelp]
Meter=String
MeterStyle=ThemeUpdaterMode
FontSize=10
FontColor=#ForegroundColor#80
ClipString=2
W=340
H=50
Y=120
Text=#tThemeUpdaterHelp#

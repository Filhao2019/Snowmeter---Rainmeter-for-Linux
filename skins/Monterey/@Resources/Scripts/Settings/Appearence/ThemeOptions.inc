[ThemeOptions]
Meter=Shape
Shape=Rectangle 20,20,(#WidgetWidth# - #SidebarWidth# - 40),80,6 | Fill Color #ForegroundColor#05 | StrokeWidth 0
X=#SidebarWidth#
Y=330
DynamicVariables=1
Hidden=([&ThemeAsNumber]=5)

[ThemeBackgroundLabel]
Meter=String
MeterStyle=ThemeLabel
Text=#tBackgroundColor#
Y=360
Hidden=([&ThemeAsNumber]<>1) && ([&ThemeAsNumber]<>2) && ([&ThemeAsNumber]<>4)

[LightBackgroundValue]
Meter=String
MeterStyle=ThemeLabel
FontFace=Consolas
FontSize=10
Text=#LightThemeBackground# ▼
Padding=2,2,2,2
StringAlign=Right
Y=(360 - 2)
X=(#WidgetWidth# - 30)
LeftMouseUpAction=[!CommandMeasure "LightBackgroundInput" "ExecuteBatch ALL"]
Hidden=([&ThemeAsNumber]<>1)

[LightBackgroundInput]
Measure=Plugin
Plugin=InputText
DefaultValue=#LightThemeBackground#
FontFace=Consolas
FontSize=10
FontColor=#ForegroundColor#
SolidColor=#BackgroundColor#
Y=(360 - 2)
X=(#WidgetWidth# - 30 - 70)
W=70
StringAlign=Center
DynamicVariables=1
Command1=[!WriteKeyValue Variables LightThemeBackground "$UserInput$" "#@#Variables\Global.inc"][!RefreshGroup Monterey]
Hidden=([&ThemeAsNumber]<>1)

[Line3]
Meter=Shape
Shape=Rectangle (#SidebarWidth# + 20),(390),(#WidgetWidth#-#SidebarWidth#-40),0 | StrokeWidth 0.5 | Stroke Color 80808080
Hidden=([&ThemeAsNumber]=5)

[ThemeForegroundLabel]
Meter=String
MeterStyle=ThemeLabel
Text=#tForegroundColor#
Y=400
Hidden=([&ThemeAsNumber]<>1) && ([&ThemeAsNumber]<>2) && ([&ThemeAsNumber]<>4)

[LightForegroundValue]
Meter=String
MeterStyle=ThemeLabel
FontFace=Consolas
Text=#LightThemeForeground# ▼
FontSize=10
Padding=2,2,2,2
StringAlign=Right
Y=(400 - 2)
X=(#WidgetWidth# - 30)
LeftMouseUpAction=[!CommandMeasure "LightForegroundInput" "ExecuteBatch ALL"]
Hidden=([&ThemeAsNumber]<>1)

[LightForegroundInput]
Measure=Plugin
Plugin=InputText
DefaultValue=#LightThemeForeground#
FontFace=Consolas
FontSize=10
FontColor=#ForegroundColor#
SolidColor=#BackgroundColor#
Y=(400 - 2)
X=(#WidgetWidth# - 30 - 70)
W=70
StringAlign=Center
DynamicVariables=1
Command1=[!WriteKeyValue Variables LightThemeForeground "$UserInput$" "#@#Variables\Global.inc"][!RefreshGroup Monterey]
Hidden=([&ThemeAsNumber]<>1)

[DarkBackgroundValue]
Meter=String
MeterStyle=ThemeLabel
FontFace=Consolas
Text=#DarkThemeBackground# ▼
FontSize=10
Padding=2,2,2,2
StringAlign=Right
Y=(360 - 2)
X=(#WidgetWidth# - 30)
LeftMouseUpAction=[!CommandMeasure "DarkBackgroundInput" "ExecuteBatch ALL"]
Hidden=([&ThemeAsNumber]<>2)

[DarkBackgroundInput]
Measure=Plugin
Plugin=InputText
DefaultValue=#DarkThemeBackground#
FontFace=#FontFace#
FontSize=10
FontColor=#ForegroundColor#
SolidColor=#BackgroundColor#
Y=(360 - 2)
X=(#WidgetWidth# - 30 - 70)
W=70
StringAlign=Center
DynamicVariables=1
Command1=[!WriteKeyValue Variables DarkThemeBackground "$UserInput$" "#@#Variables\Global.inc"][!RefreshGroup Monterey]
Hidden=([&ThemeAsNumber]<>2)

[DarkForegroundValue]
Meter=String
MeterStyle=ThemeLabel
FontFace=Consolas
Text=#DarkThemeForeground# ▼
FontSize=10
Padding=2,2,2,2
StringAlign=Right
Y=(400 - 2)
X=(#WidgetWidth# - 30)
LeftMouseUpAction=[!CommandMeasure "DarkForegroundInput" "ExecuteBatch ALL"]
Hidden=([&ThemeAsNumber]<>2)

[DarkForegroundInput]
Measure=Plugin
Plugin=InputText
DefaultValue=#DarkThemeForeground#
FontFace=Consolas
FontFace=#FontFace#
FontSize=10
FontColor=#ForegroundColor#
SolidColor=#BackgroundColor#
Y=(400 - 2)
X=(#WidgetWidth# - 30 - 70)
W=70
StringAlign=Center
DynamicVariables=1
Command1=[!WriteKeyValue Variables DarkThemeForeground "$UserInput$" "#@#Variables\Global.inc"][!RefreshGroup Monterey]
Hidden=([&ThemeAsNumber]<>2)

[AutoThemeStartLabel]
Meter=String
MeterStyle=ThemeLabel
Text=#tDarkTheme# #tTheme# #tFrom#
Y=360
Hidden=([&ThemeAsNumber]<>3)

[AutoThemeStartValue]
Meter=String
MeterStyle=ThemeLabel
FontWeight=100
Text=#DarkThemeFrom#    ▼
FontSize=10
Padding=2,2,2,2
StringAlign=Right
Y=(360 - 2)
X=(#WidgetWidth# - 30)
W=76
LeftMouseUpAction=[!CommandMeasure "AutoThemeStartInput" "ExecuteBatch ALL"]
Hidden=([&ThemeAsNumber]<>3)

[AutoThemeStartInput]
Measure=Plugin
Plugin=InputText
DefaultValue=#DarkThemeFrom#
FontFace=#FontFace#
FontSize=10
FontColor=#ForegroundColor#
SolidColor=#BackgroundColor#
Y=(360 - 2)
X=(#WidgetWidth# - 30 - 70)
W=70
StringAlign=Center
DynamicVariables=1
Command1=[!WriteKeyValue Variables DarkThemeFrom "$UserInput$" "#@#Variables\Global.inc"][!RefreshGroup Monterey]
Hidden=([&ThemeAsNumber]<>3)

[AutoThemeEndLabel]
Meter=String
MeterStyle=ThemeLabel
Text=#tTo#
Y=400
Hidden=([&ThemeAsNumber]<>3)

[AutoThemeEndValue]
Meter=String
MeterStyle=ThemeLabel
Text=#DarkThemeTo#    ▼
FontWeight=100
FontSize=10
Padding=2,2,2,2
StringAlign=Right
Y=(400 - 2)
X=(#WidgetWidth# - 30)
LeftMouseUpAction=[!CommandMeasure "AutoThemeEndInput" "ExecuteBatch ALL"]
Hidden=([&ThemeAsNumber]<>3)

[AutoThemeEndInput]
Measure=Plugin
Plugin=InputText
DefaultValue=#DarkThemeTo#
FontFace=#FontFace#
FontSize=10
FontColor=#ForegroundColor#
SolidColor=#BackgroundColor#
Y=(400 - 2)
X=(#WidgetWidth# - 30 - 70)
W=70
StringAlign=Center
DynamicVariables=1
Command1=[!WriteKeyValue Variables DarkThemeTo "$UserInput$" "#@#Variables\Global.inc"][!RefreshGroup Monterey]
Hidden=([&ThemeAsNumber]<>3)

[ColorBackgroundValue]
Meter=String
MeterStyle=ThemeLabel
Text=#ColorThemeBackground# ▼
FontFace=Consolas
FontWeight=100
FontSize=10
Padding=2,2,2,2
StringAlign=Right
Y=(360 - 2)
X=(#WidgetWidth# - 30)
LeftMouseUpAction=[!CommandMeasure "ColorBackgroundInput" "ExecuteBatch ALL"]
Hidden=([&ThemeAsNumber]<>4)

[ColorBackgroundInput]
Measure=Plugin
Plugin=InputText
DefaultValue=#ColorThemeBackground#
FontFace=Consolas
FontSize=10
FontColor=#ForegroundColor#
SolidColor=#BackgroundColor#
Y=(360 - 2)
X=(#WidgetWidth# - 30 - 70)
W=70
StringAlign=Center
DynamicVariables=1
Command1=[!WriteKeyValue Variables ColorThemeBackground "$UserInput$" "#@#Variables\Global.inc"][!RefreshGroup Monterey]
Hidden=([&ThemeAsNumber]<>4)

[ColorForegroundValue]
Meter=String
MeterStyle=ThemeLabel
FontFace=Consolas
Text=#ColorThemeForeground# ▼
FontSize=10
Padding=2,2,2,2
StringAlign=Right
Y=(400 - 2)
X=(#WidgetWidth# - 30)
LeftMouseUpAction=[!CommandMeasure "ColorForegroundInput" "ExecuteBatch ALL"]
Hidden=([&ThemeAsNumber]<>4)

[ColorForegroundInput]
Measure=Plugin
Plugin=InputText
DefaultValue=#ColorThemeForeground#
FontFace=#FontFace#
FontSize=10
FontColor=#ForegroundColor#
SolidColor=#BackgroundColor#
Y=(400 - 2)
X=(#WidgetWidth# - 30 - 70)
W=70r
StringAlign=Center
DynamicVariables=1
Command1=[!WriteKeyValue Variables ColorThemeForeground "$UserInput$" "#@#Variables\Global.inc"][!RefreshGroup Monterey]
Hidden=([&ThemeAsNumber]<>4)

[Line3]
Meter=Shape
Shape=Rectangle (#SidebarWidth# + 20),(225),(#WidgetWidth#-#SidebarWidth#-40),0 | StrokeWidth 0.5 | Stroke Color 80808080

[ColorThemeHelp]
Meter=String
MeterStyle=ThemeLabel
FontSize=10
FontColor=#ForegroundColor#80
W=300
Y=440
Text=#tColorThemeHelp#
Hidden=([&ThemeAsNumber]<>4)
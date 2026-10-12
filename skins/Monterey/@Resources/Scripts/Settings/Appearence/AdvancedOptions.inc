[AdvancedOptions]
Meter=Shape
Shape=Rectangle 20,20,(#WidgetWidth# - #SidebarWidth# - 40),(120 - 40 * Floor([&ThemeAsNumber] / 5)),6 | Fill Color #ForegroundColor#05 | StrokeWidth 0
X=#SidebarWidth#
Y=190
DynamicVariables=1

[Label1]
Meter=String
MeterStyle=ThemeLabel
Text=#tWidgetSize#
W=350
Y=(220)

[Value1]
Meter=String
MeterStyle=ThemeLabel
FontWeight=100
Text=#WidgetBase#    ▼
FontSize=10
Padding=2,2,2,2
StringAlign=Right
Y=(220 - 2)
X=(#WidgetWidth# - 30)
LeftMouseUpAction=[!CommandMeasure "Input1" "ExecuteBatch ALL"]

[Input1]
Measure=Plugin
Plugin=InputText
DefaultValue=#WidgetBase#
FontFace=#FontFace#
FontSize=10
FontColor=#ForegroundColor#
SolidColor=#BackgroundColor#
Y=(220 - 2)
X=(#WidgetWidth# - 30 - 60)
W=60
StringAlign=Center
DynamicVariables=1
Command1=[!WriteKeyValue Variables WidgetBase "$UserInput$" "#@#Variables\Global.inc"][!RefreshGroup Monterey]

[Line1]
Meter=Shape
Shape=Rectangle (#SidebarWidth# + 20),(250),(#WidgetWidth#-#SidebarWidth#-40),0 | StrokeWidth 0.5 | Stroke Color 80808080

[Label2]
Meter=String
MeterStyle=ThemeLabel
Text=#tWidgetPadding#
W=350
Y=(260)

[Value2]
Meter=String
MeterStyle=ThemeLabel
Text=#PaddingBase#    ▼
FontWeight=100
FontSize=10
Padding=2,2,2,2
StringAlign=Right
Y=(260 - 2)
X=(#WidgetWidth# - 30)
W=76
LeftMouseUpAction=[!CommandMeasure "Input2" "ExecuteBatch ALL"]

[Input2]
Measure=Plugin
Plugin=InputText
DefaultValue=#PaddingBase#
FontFace=#FontFace#
FontSize=10
FontColor=#ForegroundColor#
SolidColor=#BackgroundColor#
Y=(260 - 2)
X=(#WidgetWidth# - 30 - 60)
W=60
StringAlign=Center
DynamicVariables=1
Command1=[!WriteKeyValue Variables PaddingBase "$UserInput$" "#@#Variables\Global.inc"][!RefreshGroup Monterey]

[Line2]
Meter=Shape
Shape=Rectangle (#SidebarWidth# + 20),(290),(#WidgetWidth#-#SidebarWidth#-40),0 | StrokeWidth 0.5 | Stroke Color 80808080
Hidden=([&ThemeAsNumber]=5)

[Label3]
Meter=String
MeterStyle=ThemeLabel
Text=#tWidgetRadius#
Hidden=([&ThemeAsNumber]=5)
W=350
Y=300

[Value3]
Meter=String
MeterStyle=ThemeLabel
Text=#RadiusBase#    ▼
FontWeight=100
FontSize=10
Padding=2,2,2,2
StringAlign=Right
Y=(300 - 2)
X=(#WidgetWidth# - 30)
W=76
LeftMouseUpAction=[!CommandMeasure "Input3" "ExecuteBatch ALL"]
Hidden=([&ThemeAsNumber]=5)

[Input3]
Measure=Plugin
Plugin=InputText
DefaultValue=#RadiusBase#
FontFace=#FontFace#
FontSize=10
FontColor=#ForegroundColor#
SolidColor=#BackgroundColor#
Y=(300 - 2)
X=(#WidgetWidth# - 30 - 60)
W=60
StringAlign=Center
DynamicVariables=1
Command1=[!WriteKeyValue Variables RadiusBase "$UserInput$" "#@#Variables\Global.inc"][!RefreshGroup Monterey]
Hidden=([&ThemeAsNumber]=5)
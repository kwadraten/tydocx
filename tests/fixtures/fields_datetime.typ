Auto: #context datetime.today().display()

Iso: #context datetime.today().display("[year]-[month]-[day]")

Slashed: #context datetime.today().display("[year]/[month]/[day]")

Long: #context datetime.today().display("[month repr:long] [day padding:none], [year repr:last_two]")

Short: #context datetime.today().display("[month repr:short] [day padding:none], [year]")

Weekday: #context datetime.today().display("[weekday], [day padding:none] [month repr:long]")

Ordinal: #context datetime.today().display("[ordinal]")

#let fmt = "[month]/[day]"
Computed: #context datetime.today().display(fmt)

Fixed: #datetime(year: 2026, month: 3, day: 4).display("[year]-[month]-[day]")

Fixed time: #context datetime(hour: 14, minute: 30, second: 0).display()

### Sicherheit
- „Lokal öffnen“: Nach einem Absturz oder Beenden der App liegengebliebene
  Kopien von Server-Dateien werden beim nächsten Start entfernt, statt
  dauerhaft im Klartext im Cache zu bleiben.
  Kopien, die eine frühere Version liegen gelassen hat, liegen direkt in
  `<cache>/smart-ssh/edit-sessions/` und bleiben dort, bis man sie selbst
  löscht; die Ordner `data-…` darin verwaltet die App.

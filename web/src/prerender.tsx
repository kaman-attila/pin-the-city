import { renderToString } from 'react-dom/server'
import AccountMenu from './AccountMenu'
import Archive from './Archive'
import Menu from './Menu'
import Privacy from './Privacy'

const noop = () => {}

const account = (
  <AccountMenu
    profile={null}
    settings={{ theme: 'system', units: 'metric' }}
    onSettingsChange={noop}
    onLoggedIn={async () => {}}
    onLoggedOut={noop}
  />
)

export const pages = {
  menu: () =>
    renderToString(
      <Menu
        account={account}
        bestScore={null}
        streak={0}
        onDaily={noop}
        onFreeplay={noop}
        onArchive={noop}
        onPrivacy={noop}
      />,
    ),
  privacy: () => renderToString(<Privacy account={account} onHome={noop} />),
  archive: () => renderToString(<Archive account={account} onHome={noop} onOpen={noop} />),
}

// Preserve ssh-connection's deferred SFTP boundary without presenting the
// shared implementation itself as a code-splitting target to Rollup.
export { fastGetViaSftp } from '../providers/ssh-filesystem-provider-sftp'

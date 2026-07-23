{-# LANGUAGE LambdaCase #-}
{-# LANGUAGE OverloadedStrings #-}

module Main (main) where

import Control.Concurrent.MVar
import Control.Carrier.Accum.Strict (evalAccum)
import Control.Carrier.Throw.Either (runThrow)
import Control.Lens hiding ((.=))
import Control.Monad (foldM, unless, when)
import Control.Monad.State.Strict (execState, execStateT)
import Data.Aeson
import Data.Aeson.KeyMap qualified as KM
import Data.Aeson.Types (Parser)
import Data.ByteString.Lazy qualified as LBS
import Data.ByteString.Lazy.Char8 qualified as LBS8
import Data.Int (Int64)
import Data.IntMap.Strict qualified as IM
import Data.Maybe (fromMaybe)
import Data.Sequence (Seq)
import Data.Text (Text)
import Data.Text qualified as T
import Data.Text.Encoding qualified as TE
import Data.Yaml (decodeEither', parseEither)
import Network.HTTP.Types (hContentType, status200)
import Network.Wai
import Network.Wai.Handler.Warp qualified as Warp
import Swarm.Effect (runMetricIO, runTimeIO)
import Swarm.Failure (SystemFailure)
import Swarm.Game.CESK (continue)
import Swarm.Game.Entity (Inventory, elems, entityName)
import Swarm.Game.Robot
import Swarm.Game.Robot.Concrete
import Swarm.Game.Scenario
import Swarm.Game.Scenario.Status (ScenarioWith (..), emptyLaunchParams)
import Swarm.Game.State
import Swarm.Game.State.Initialize (scenarioToGameState)
import Swarm.Game.State.Runtime
import Swarm.Game.State.Substate
import Swarm.Game.Step (gameTick)
import Swarm.Game.Step.Validate (badErrorsInLogs)
import Swarm.Game.Tick (TickNumber (..))
import Swarm.Language.Pipeline (processTerm')
import Swarm.Language.Syntax (sType)
import Swarm.Language.Value (emptyEnv)
import Swarm.Log (logToText)
import Swarm.Util.Yaml (parseJSONE')
import System.Environment (getArgs, lookupEnv)
import System.Exit (die, exitFailure)
import System.IO.Error (isDoesNotExistError, tryIOError)
import Text.Read (readMaybe)

apiVersion :: Text
apiVersion = "swarm-api-v1"

auditVersion :: Text
auditVersion = "swarm-audit-v1"

maxAdvancePerCall :: Int64
maxAdvancePerCall = 100000

data Command
  = Status
  | RunProgram Text
  | Advance Int64
  | Submit
  | Invalid Text
  deriving (Eq, Show)

data AuditRecord = AuditRecord
  { auditSequence :: Int
  , auditCommand :: Command
  , auditResponse :: Value
  }
  deriving (Eq, Show)

data ServerState = ServerState
  { serverGame :: GameState
  , serverSequence :: Int
  }

data Config = Config
  { configScenario :: FilePath
  , configAudit :: FilePath
  , configDeadline :: Int64
  }

main :: IO ()
main = do
  args <- getArgs
  case args of
    ["serve", scenario, audit, portText, deadlineText] -> do
      port <- parsePositive "port" portText
      deadline <- parsePositive64 "deadline" deadlineText
      serve (Config scenario audit deadline) port
    ["verify", scenario, audit, deadlineText] -> do
      deadline <- parsePositive64 "deadline" deadlineText
      verifyAudit (Config scenario audit deadline)
    ["oracle", scenario, program, deadlineText] -> do
      deadline <- parsePositive64 "deadline" deadlineText
      runOracle (Config scenario "" deadline) program
    _ ->
      die $
        unlines
          [ "usage:"
          , "  swarm-harbor serve SCENARIO AUDIT PORT DEADLINE_TICKS"
          , "  swarm-harbor verify SCENARIO AUDIT DEADLINE_TICKS"
          , "  swarm-harbor oracle SCENARIO PROGRAM DEADLINE_TICKS"
          ]

parsePositive :: String -> String -> IO Int
parsePositive label input =
  case readMaybe input of
    Just n | n > 0 -> pure n
    _ -> die $ label <> " must be a positive integer"

parsePositive64 :: String -> String -> IO Int64
parsePositive64 label input =
  case readMaybe input of
    Just n | n > 0 -> pure n
    _ -> die $ label <> " must be a positive integer"

loadGame :: FilePath -> IO GameState
loadGame scenarioPath = do
  scenarioBytes <- LBS.readFile scenarioPath
  runtimeResult <-
    ( runThrow
        . evalAccum (mempty :: Seq SystemFailure)
        . initRuntimeState
        $ RuntimeOptions True False False
    ) ::
      IO (Either SystemFailure RuntimeState)
  runtime <-
    either
      (die . ("failed to initialize Swarm runtime: " <>) . show)
      pure
      runtimeResult
  raw <-
    either
      (die . ("failed to decode Swarm scenario: " <>) . show)
      pure
      (decodeEither' $ LBS.toStrict scenarioBytes)
  let inputs = gsiScenarioInputs . initState $ runtime ^. stdGameConfigInputs
  scenario <-
    either
      (die . ("failed to parse Swarm scenario: " <>))
      pure
      (parseEither (parseJSONE' inputs) raw)
  scenarioToGameState
    (ScenarioWith scenario Nothing)
    emptyLaunchParams
    Nothing
    Nothing
    runtime

serve :: Config -> Int -> IO ()
serve config port = do
  game <- loadGame $ configScenario config
  let header =
        object
          [ "schema" .= auditVersion
          , "api_version" .= apiVersion
          , "deadline_ticks" .= configDeadline config
          , "initial_state" .= snapshot config game
          ]
  LBS.writeFile (configAudit config) $ encode header <> "\n"
  appendRecorderEvent $
    object
      [ "schema" .= ("benchmark-observer-event-v1" :: Text)
      , "sequence" .= (0 :: Int)
      , "type" .= ("sidecar_started" :: Text)
      , "command" .= ("baseline" :: Text)
      , "response" .= object ["data" .= snapshot config game]
      , "score" .= scoreFor (configDeadline config) game
      , "score_delta" .= (0 :: Int)
      ]
  state <- newMVar $ ServerState game 0
  putStrLn $ "swarm-harbor listening on port " <> show port
  Warp.runSettings
    (Warp.setHost "0.0.0.0" $ Warp.setPort port Warp.defaultSettings)
    (application config state)

application :: Config -> MVar ServerState -> Application
application config state request respond = do
  body <- strictRequestBody request
  let command = parseRequest request body
  response <-
    modifyMVar state $ \old -> do
      restored <- restoreAuditIfAhead config old
      (newGame, result) <- applyCommand config command (serverGame restored)
      let sequenceNumber = serverSequence restored + 1
          record = AuditRecord sequenceNumber command result
          newState = ServerState newGame sequenceNumber
      LBS.appendFile (configAudit config) $ encode record <> "\n"
      appendRecorderEvent $
        object
          [ "schema" .= ("benchmark-observer-event-v1" :: Text)
          , "sequence" .= sequenceNumber
          , "type" .= ("request" :: Text)
          , "command" .= commandName command
          , "argument" .= commandArgument command
          , "response" .= result
          , "score" .= scoreFor (configDeadline config) newGame
          , "score_delta"
              .= ( scoreFor (configDeadline config) newGame
                     - scoreFor (configDeadline config) (serverGame restored)
                 )
          ]
      pure (newState, result)
  respond $
    responseLBS
      status200
      [(hContentType, "application/json; charset=utf-8")]
      (encode response <> "\n")

restoreAuditIfAhead :: Config -> ServerState -> IO ServerState
restoreAuditIfAhead config current = do
  contents <- LBS8.lines <$> LBS.readFile (configAudit config)
  case contents of
    [] -> verificationFailure "live audit is empty"
    headerLine : recordLines
      | length recordLines <= serverSequence current -> pure current
      | otherwise -> do
          header <-
            either (verificationFailure . ("invalid live audit header: " <>)) pure $
              eitherDecode headerLine
          validateHeader config header
          records <-
            mapM
              (either (verificationFailure . ("invalid live audit record: " <>)) pure . eitherDecode)
              recordLines
          initial <- loadGame $ configScenario config
          (sequenceNumber, restored) <-
            foldM (replayRecord config) (0, initial) records
          putStrLn $
            "restored live audit through sequence " <> show sequenceNumber
          pure $ ServerState restored sequenceNumber

parseRequest :: Request -> LBS.ByteString -> Command
parseRequest request body =
  case (requestMethod request, pathInfo request) of
    ("GET", ["v1", "status"]) -> Status
    ("GET", ["v1", "submit"]) -> Submit
    ("POST", ["v1", "run"]) ->
      case TE.decodeUtf8' $ LBS.toStrict body of
        Left _ -> Invalid "program source must be UTF-8"
        Right source
          | T.length source > 65536 -> Invalid "program source exceeds 65536 characters"
          | otherwise -> RunProgram source
    ("POST", ["v1", "advance", amount]) ->
      case readMaybe $ T.unpack amount of
        Just n | n > 0 && n <= maxAdvancePerCall -> Advance n
        _ -> Invalid "advance ticks must be between 1 and 100000"
    _ -> Invalid "unknown endpoint"

applyCommand :: Config -> Command -> GameState -> IO (GameState, Value)
applyCommand config command game =
  case command of
    Status -> pure $ success config "status" game
    Submit -> pure $ success config "submit" game
    Invalid message -> pure $ failure config "invalid" "invalid_request" message game
    RunProgram source
      | game ^. gameControls . replWorking ->
          pure $ failure config "run" "program_running" "the base robot is still running a program" game
      | otherwise ->
          let environment = fromMaybe emptyEnv $ game ^? baseEnv
           in case processTerm' environment source of
                Left err -> pure $ failure config "run" "invalid_program" err game
                Right Nothing -> pure $ failure config "run" "empty_program" "program source is empty" game
                Right (Just term) -> do
                  let started =
                        game
                          & gameControls . replStatus .~ REPLWorking (term ^. sType) Nothing
                          & baseRobot . machine %~ continue term
                      activated = execState (zoomRobots $ activateRobot 0) started
                  pure $ success config "run" activated
    Advance requested -> do
      advanced <- advanceGame (configDeadline config) requested game
      pure $ success config "advance" advanced

success :: Config -> Text -> GameState -> (GameState, Value)
success config command state =
  ( state
  , object
      [ "api_version" .= apiVersion
      , "ok" .= True
      , "command" .= command
      , "data" .= snapshot config state
      ]
  )

failure :: Config -> Text -> Text -> Text -> GameState -> (GameState, Value)
failure config command code message state =
  ( state
  , object
      [ "api_version" .= apiVersion
      , "ok" .= False
      , "command" .= command
      , "error" .= object ["code" .= code, "message" .= message]
      , "data" .= snapshot config state
      ]
  )

advanceGame :: Int64 -> Int64 -> GameState -> IO GameState
advanceGame deadline requested initial =
  settleRepl <$> execStateT loop initial
 where
  loop = do
    current <- use $ temporal . ticks . to getTickNumber
    won <- use $ winCondition . to winTick
    when (current < deadline && won == Nothing && current < target) $ do
      _ <- runMetricIO (runTimeIO gameTick)
      loop
  initialTick = getTickNumber $ initial ^. temporal . ticks
  target = min deadline $ initialTick + requested

settleRepl :: GameState -> GameState
settleRepl =
  gameControls . replStatus %~ \case
    REPLWorking ty (Just value) -> REPLDone $ Just (ty, value)
    other -> other

winTick :: WinCondition -> Maybe Int64
winTick = \case
  WinConditions (Won _ (TickNumber tick)) _ -> Just tick
  _ -> Nothing

snapshot :: Config -> GameState -> Value
snapshot config game =
  object
    [ "tick" .= current
    , "deadline_ticks" .= deadline
    , "won" .= maybe False (const True) completed
    , "win_tick" .= completed
    , "score" .= scoreFor deadline game
    , "program_running" .= (game ^. gameControls . replWorking)
    , "objectives" .= (game ^. winCondition)
    , "robots" .= fmap robotSummary visibleRobots
    , "errors" .= badErrorsInLogs game
    ]
 where
  current = getTickNumber $ game ^. temporal . ticks
  deadline = configDeadline config
  completed = winTick $ game ^. winCondition
  visibleRobots =
    filter ((/= "seed") . view robotName) $
      IM.elems $
        game ^. robotInfo . robotMap

robotSummary :: Robot -> Value
robotSummary robot =
  object
    [ "id" .= (robot ^. robotID)
    , "name" .= (robot ^. robotName)
    , "location" .= (robot ^. robotLocation)
    , "orientation" .= (robot ^. robotOrientation)
    , "active" .= isActive robot
    , "waiting_until" .= fmap getTickNumber (waitingUntil robot)
    , "inventory" .= inventorySummary (robot ^. robotInventory)
    , "devices" .= inventorySummary (robot ^. equippedDevices)
    , "log" .= takeLast 20 (logToText $ robot ^. robotLog)
    ]

inventorySummary :: Inventory -> [Value]
inventorySummary inventory =
  [ object ["count" .= count, "name" .= (entity ^. entityName)]
  | (count, entity) <- elems inventory
  , count > 0
  ]

takeLast :: Int -> [a] -> [a]
takeLast count = reverse . take count . reverse

appendRecorderEvent :: Value -> IO ()
appendRecorderEvent value = do
  configured <- lookupEnv "BENCHMARK_OBSERVER_INBOX"
  let path = fromMaybe "/logs/artifacts/observer/game-inbox.jsonl" configured
  result <- tryIOError $ LBS.appendFile path $ encode value <> "\n"
  case result of
    Right () -> pure ()
    Left err | isDoesNotExistError err -> pure ()
    Left err -> ioError err

scoreFor :: Int64 -> GameState -> Int64
scoreFor deadline game =
  case winTick $ game ^. winCondition of
    Just tick | tick <= deadline -> deadline + 1 - tick
    _ -> 0

instance ToJSON AuditRecord where
  toJSON record =
    object
      [ "sequence" .= auditSequence record
      , "command" .= commandName (auditCommand record)
      , "argument" .= commandArgument (auditCommand record)
      , "response" .= auditResponse record
      ]

instance FromJSON AuditRecord where
  parseJSON = withObject "AuditRecord" $ \value -> do
    sequenceNumber <- value .: "sequence"
    name <- value .: "command"
    argValue <- value .:? "argument" .!= Null
    response <- value .: "response"
    command <- parseCommand name argValue
    pure $ AuditRecord sequenceNumber command response

commandName :: Command -> Text
commandName = \case
  Status -> "status"
  RunProgram _ -> "run"
  Advance _ -> "advance"
  Submit -> "submit"
  Invalid _ -> "invalid"

commandArgument :: Command -> Value
commandArgument = \case
  Status -> Null
  RunProgram source -> object ["source" .= source]
  Advance ticksToRun -> object ["ticks" .= ticksToRun]
  Submit -> Null
  Invalid message -> object ["message" .= message]

parseCommand :: Text -> Value -> Parser Command
parseCommand name argValue =
  case name of
    "status" -> pure Status
    "run" -> withObject "run argument" (fmap RunProgram . (.: "source")) argValue
    "advance" -> withObject "advance argument" (fmap Advance . (.: "ticks")) argValue
    "submit" -> pure Submit
    "invalid" -> withObject "invalid argument" (fmap Invalid . (.: "message")) argValue
    _ -> fail "unknown audit command"

verifyAudit :: Config -> IO ()
verifyAudit config = do
  contents <- LBS8.lines <$> LBS.readFile (configAudit config)
  case contents of
    [] -> verificationFailure "audit is empty"
    headerLine : recordLines -> do
      header <-
        either (verificationFailure . ("invalid audit header: " <>)) pure $
          eitherDecode headerLine
      validateHeader config header
      records <-
        mapM
          (either (verificationFailure . ("invalid audit record: " <>)) pure . eitherDecode)
          recordLines
      initial <- loadGame $ configScenario config
      final <- foldM (replayRecord config) (0, initial) records
      let game = snd final
          score = scoreFor (configDeadline config) game
          ticksUsed = getTickNumber $ game ^. temporal . ticks
      putStrLn $ "score: " <> show score
      putStrLn $ "ticks: " <> show ticksUsed
      putStrLn $ "won: " <> show (winTick (game ^. winCondition) /= Nothing)

validateHeader :: Config -> Value -> IO ()
validateHeader config = \case
  Object value
    | KM.lookup "schema" value == Just (String auditVersion)
    , KM.lookup "api_version" value == Just (String apiVersion)
    , KM.lookup "deadline_ticks" value == Just (toJSON $ configDeadline config) ->
        pure ()
  _ -> verificationFailure "audit header does not match the task configuration"

replayRecord :: Config -> (Int, GameState) -> AuditRecord -> IO (Int, GameState)
replayRecord config (previousSequence, game) record = do
  let expectedSequence = previousSequence + 1
  unless (auditSequence record == expectedSequence) $
    verificationFailure "audit sequence is not contiguous"
  (nextGame, replayedResponse) <- applyCommand config (auditCommand record) game
  unless (replayedResponse == auditResponse record) $
    verificationFailure $
      "replay diverged at audit sequence " <> show expectedSequence
  pure (expectedSequence, nextGame)

verificationFailure :: String -> IO a
verificationFailure message = do
  putStrLn $ "verification error: " <> message
  exitFailure

runOracle :: Config -> FilePath -> IO ()
runOracle config programPath = do
  initial <- loadGame $ configScenario config
  source <- T.pack <$> readFile programPath
  (started, response) <- applyCommand config (RunProgram source) initial
  case response of
    Object value | KM.lookup "ok" value == Just (Bool True) -> do
      final <- advanceGame (configDeadline config) (configDeadline config) started
      LBS8.putStrLn $ encode $ snapshot config final
      putStrLn $ "score: " <> show (scoreFor (configDeadline config) final)
    _ -> do
      LBS8.putStrLn $ encode response
      exitFailure

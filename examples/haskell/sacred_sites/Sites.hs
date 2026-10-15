{-# LANGUAGE ForeignFunctionInterface #-}

-- | The sacred sites of Age of Empires 4, as signals: a small game seen from above through an
-- orthographic camera, to watch from outside while it runs.
--
-- > cargo run --bin mira-play -- examples/haskell/sacred_sites              # the game
-- > cargo run --bin mira-play -- examples/haskell/sacred_sites --headless   # the same with no window
-- > cargo run --bin mira-debug -- watch           # in another terminal: the signal graph, live
-- > cargo run --bin mira-debug -- signal_set name=win_after value=20
-- > cargo run --bin mira-play -- examples/haskell/sacred_sites --rules sites.rules   # the rules kept as text
--
-- Red holds both sites, and wins when its clock reaches @win_after@. The clock stops while a
-- blue unit stands on any site red holds. Two blue scouts wander on and off the sites; no
-- code ever sets the clock or a paused flag. See @docs/SIGNALS.md@.
--
-- On screen: the sites are slabs, red while red's clock runs and amber while a scout is on
-- one; the bar along the bottom is the clock, filling toward @win_after@. All of it is drawn
-- from the signals, so what you force or rewire from outside shows at once.
module Sites where

import Control.Monad (unless, when)
import Data.IORef (modifyIORef', newIORef, readIORef, writeIORef)
import Data.Int (Int32)
import Data.Maybe (fromMaybe)
import Foreign (Ptr, Storable (..), castPtr)
import Foreign.C.Types (CInt (..))
import Mira
import Text.Printf (printf)

-- | Which side a unit is on, or holds a site: nobody, red or blue.
newtype Team = Team Int32
  deriving (Eq)

nobody, red, blue :: Team
nobody = Team 0
red = Team 1
blue = Team 2

instance Storable Team where
  sizeOf _ = 4
  alignment _ = 4
  peek ptr = Team <$> peek (castPtr ptr)
  poke ptr (Team team) = poke (castPtr ptr) team

-- | A site, and who holds it.
newtype Site = Site Team

instance Storable Site where
  sizeOf _ = 4
  alignment _ = 4
  peek ptr = Site <$> peek (castPtr ptr)
  poke ptr (Site holder) = poke (castPtr ptr) holder

-- | Walks between the sites' neighbourhood and the middle of the map: the seconds one trip
-- out and back takes, and where it turns round.
data Scout = Scout !Float !Float

instance Storable Scout where
  sizeOf _ = 8
  alignment _ = 4
  peek ptr = Scout <$> peekByteOff ptr 0 <*> peekByteOff ptr 4
  poke ptr (Scout period reach) = pokeByteOff ptr 0 period >> pokeByteOff ptr 4 reach

-- | Marks the bar that shows red's clock.
newtype ClockBar = ClockBar Int32

instance Storable ClockBar where
  sizeOf _ = 4
  alignment _ = 4
  peek ptr = ClockBar <$> peek (castPtr ptr)
  poke ptr (ClockBar mark) = poke (castPtr ptr) mark

siteRadius, barLength :: Float
siteRadius = 2
barLength = 24

foreign export ccall "mira_hs_main" pluginMain :: Ptr () -> IO CInt

pluginMain :: Ptr () -> IO CInt
pluginMain = plugin $ \app -> do
  transform <- lookupComponent app "mira.Transform"
  team <- registerComponent app "sites.Team"
  site <- registerComponent app "sites.Site"
  scout <- registerComponent app "sites.Scout"
  bar <- registerComponent app "sites.ClockBar"
  describeComponent app team [Field "team" FieldI32 1 0]
  describeComponent app site [Field "holder" FieldI32 1 0]
  describeComponent app scout [Field "period" FieldF32 1 0, Field "reach" FieldF32 1 4]
  -- Whether red's win has been said already, so that it is said once.
  announced <- statePtr app "sites.announced" :: IO (Ptr Int32)

  addSystem_ app "setup" Startup $ \sys -> do
    cube <- meshCube sys 1
    ball <- meshSphere sys 0.6
    ground <- meshPlane sys 60
    let put position = do
          entity <- spawn sys
          insert sys entity transform (at position)
          pure entity
        unit position side = do
          entity <- put position
          insert sys entity team side
          setMesh sys entity ball
          setMaterial sys entity (material (if side == red then V3 0.85 0.15 0.15 else V3 0.15 0.3 0.9))
          pure entity

    -- Two sites, both red's. A site is a flat slab; `show` sizes and colours it.
    mapM_
      ( \x -> do
          entity <- put (V3 x 0 0)
          insert sys entity site (Site red)
          setMesh sys entity cube
          setMaterial sys entity (material (V3 0.7 0.1 0.1))
      )
      [-10, 10]
    -- One scout visits the east site, the other both.
    east <- unit (V3 0 0 0) blue
    insert sys east scout (Scout 16 10.5)
    both <- unit (V3 0 0 0) blue
    insert sys both scout (Scout 23 (-9))
    _ <- unit (V3 (-10) 0 0) red

    field <- put (V3 0 (-0.05) 0)
    setMesh sys field ground
    setMaterial sys field (material (V3 0.25 0.4 0.22))

    clock <- put (V3 (-barLength * 0.5) 0.3 9)
    insert sys clock bar (ClockBar 1)
    setMesh sys clock cube
    setMaterial sys clock (material (V3 0.9 0.85 0.3))

    eye <- spawn sys
    insert sys eye transform (lookingAt (V3 0 30 26) (V3 0 0 0))
    setCameraOrthographic sys eye 26 0.1 1000
    sun <- spawn sys
    insert sys sun transform (lookingAt (V3 0 0 0) (V3 (-0.4) (-1) (-0.3)))
    setLight sys sun defaultLight

    -- The rules. Each is a signal worked out from others; nothing below ever sets the clock.
    defineSignal sys "blue.calm" SignalNot ["blue.contesting"]
    defineSignal sys "red.clock.running" SignalAnd ["red.holds_all", "blue.calm"]
    defineSignal sys "red.lost_a_site" SignalNot ["red.holds_all"]
    -- A lost site sets the clock back; so does asking for a new round.
    defineSignal sys "restart" (SignalTruth False) []
    defineSignal sys "red.clock.reset" SignalOr ["red.lost_a_site", "restart"]
    defineSignal sys "red.clock" SignalTimer ["red.clock.running", "red.clock.reset"]
    defineSignal sys "win_after" (SignalNumber 60) []
    defineSignal sys "red.wins" SignalGreaterOrEqual ["red.clock", "win_after"]

  -- The facts the rules stand on, said afresh every frame from where things are.
  addSystem2 app "facts" Update ((,) <$> readC team <*> readC transform) ((,) <$> readC site <*> readC transform) $
    \sys units sites -> do
      contesting <- newIORef False
      holdsAll <- newIORef True
      forEach sites $ \_ (Site holder, centre) -> do
        unless (holder == red) (writeIORef holdsAll False)
        when (holder == red) $
          forEach units $ \_ (side, place) ->
            when (side == blue && distance (translation place) (translation centre) < siteRadius) $
              writeIORef contesting True
      setSignal sys "blue.contesting" =<< readIORef contesting
      setSignal sys "red.holds_all" =<< readIORef holdsAll

  addSystem app "wander" Update ((,) <$> readC scout <*> write transform) $ \sys _ (Scout period reach, place) -> do
    now <- realToFrac <$> elapsedSeconds sys
    modifyRef place $ \t ->
      let V3 _ y z = translation t
       in t {translation = V3 (reach * sin (now / period * 2 * pi)) y z}

  addSystem_ app "announce" Update $ \sys -> do
    won <- signalIsTrue sys "red.wins"
    said <- peek announced
    when (won && said == 0) $ do
      clock <- fromMaybe 0 <$> signal sys "red.clock"
      logInfo (printf "red wins, %.0f seconds on the clock" clock)
    poke announced (if won then 1 else 0)

  -- Draws the state of the rules: nothing here is kept, it is all read from the signals.
  -- The two reach different things: a site is never the bar.
  addSystem2 app "show" Update ((,) <$> readC site <*> write transform) (write transform <* with bar <* without site) $
    \sys sites bars -> do
      contested <- signalIsTrue sys "blue.contesting"
      forEach sites $ \entity (Site holder, place) -> do
        modifyRef place $ \t -> t {scale = V3 (siteRadius * 2) 0.1 (siteRadius * 2)}
        setMaterial sys entity . material $
          if holder == red
            then (if contested then V3 0.9 0.6 0.1 else V3 0.7 0.1 0.1)
            else (if holder == blue then V3 0.1 0.2 0.7 else V3 0.5 0.5 0.5)
      clock <- fromMaybe 0 <$> signal sys "red.clock"
      winAfter <- fromMaybe 0 <$> signal sys "win_after"
      let filled = if winAfter > 0 then realToFrac (max 0 (min 1 (clock / winAfter))) else 0 :: Float
      forEach bars $ \_ place ->
        modifyRef place $ \t ->
          let V3 _ y z = translation t
           in t
                { scale = V3 (max 0.01 (barLength * filled)) 0.4 0.6
                , translation = V3 (-barLength * 0.5 + barLength * filled * 0.5) y z
                }
      won <- signalIsTrue sys "red.wins"
      let state
            | won = "red wins"
            | contested = "clock stopped: a scout is on a site"
            | otherwise = "clock running"
      setWindowTitle sys (printf "sacred sites: %.0f of %.0f s, %s" clock winAfter (state :: String))

distance :: V3 -> V3 -> Float
distance (V3 ax ay az) (V3 bx by bz) = sqrt ((ax - bx) ^ (2 :: Int) + (ay - by) ^ (2 :: Int) + (az - bz) ^ (2 :: Int))

at :: V3 -> Transform
at position = Transform position (Quat 0 0 0 1) (V3 1 1 1)